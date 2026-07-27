//! Transaction registry for managing active transactions.
//!
//! Entries hold `sqlx::Transaction` (via `TransactionInner`), so removing an
//! entry is always safe: if nobody committed it, dropping the value rolls the
//! transaction back before the connection returns to the pool.

use crate::settings::{PoolSettings, PoolTimeoutSettings};
use crate::transaction::inner::TransactionInner;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, warn};

pub(crate) static TRANSACTION_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct TransactionRegistry {
    transactions: RwLock<HashMap<u64, Arc<Mutex<TransactionInner>>>>,
    /// Per-pool timeout settings to avoid one pool overriding another's timeouts
    pool_settings: RwLock<HashMap<String, PoolTimeoutSettings>>,
}

impl TransactionRegistry {
    pub fn new() -> Self {
        Self {
            transactions: RwLock::new(HashMap::new()),
            pool_settings: RwLock::new(HashMap::new()),
        }
    }

    pub async fn update_settings(&self, pool_name: &str, settings: &PoolSettings) {
        let mut pool_settings = self.pool_settings.write().await;
        let entry = pool_settings
            .entry(pool_name.to_string())
            .or_insert_with(PoolTimeoutSettings::default);

        if let Some(timeout) = settings.transaction_timeout {
            entry.timeout = timeout;
        }
        if let Some(interval) = settings.transaction_cleanup_interval {
            entry.cleanup_interval = interval;
        }
    }

    pub async fn get_cleanup_interval(&self) -> Duration {
        // Use the minimum cleanup interval across all pools for aggressive cleanup
        let pool_settings = self.pool_settings.read().await;
        pool_settings
            .values()
            .map(|s| s.cleanup_interval)
            .min()
            .unwrap_or_else(|| PoolTimeoutSettings::default().cleanup_interval)
    }

    pub async fn insert(&self, tx: TransactionInner) -> u64 {
        let id = TRANSACTION_ID.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.transactions.write().await;
        guard.insert(id, Arc::new(Mutex::new(tx)));
        id
    }

    pub async fn get(&self, id: u64) -> Option<Arc<Mutex<TransactionInner>>> {
        let guard = self.transactions.read().await;
        guard.get(&id).cloned()
    }

    pub async fn remove(&self, id: u64) -> Option<Arc<Mutex<TransactionInner>>> {
        let mut guard = self.transactions.write().await;
        guard.remove(&id)
    }

    /// Rollback and remove all active transactions (for shutdown).
    pub async fn rollback_all(&self) -> usize {
        let drained: Vec<(u64, Arc<Mutex<TransactionInner>>)> = {
            let mut guard = self.transactions.write().await;
            guard.drain().collect()
        };

        let mut count = 0;
        for (tx_id, tx_arc) in drained {
            count += 1;
            if let Ok(mut inner) = tx_arc.try_lock() {
                if let Err(e) = inner.rollback().await {
                    warn!(
                        "Failed to rollback transaction {} on shutdown: {}",
                        tx_id, e
                    );
                } else {
                    debug!("Rolled back transaction {} on shutdown", tx_id);
                }
            } else {
                // In use elsewhere: dropping our Arc leaves rollback to sqlx's
                // Drop once the current holder finishes.
                debug!(
                    "Transaction {} busy on shutdown; sqlx rolls it back on drop",
                    tx_id
                );
            }
        }
        count
    }

    /// Remove transactions idle past their pool's timeout, rolling them back.
    pub async fn cleanup_stale_transactions(&self) -> usize {
        let pool_timeouts: HashMap<String, Duration> = {
            let pool_settings = self.pool_settings.read().await;
            pool_settings
                .iter()
                .map(|(name, settings)| (name.clone(), settings.timeout))
                .collect()
        };

        // Phase 1: pull stale entries out of the map (hold the write lock briefly).
        // A locked entry is mid-query — that counts as activity, keep it.
        let stale: Vec<(u64, Arc<Mutex<TransactionInner>>)> = {
            let mut guard = self.transactions.write().await;
            let now = Instant::now();
            let mut stale = Vec::new();

            guard.retain(|tx_id, tx_arc| {
                let Ok(inner) = tx_arc.try_lock() else {
                    return true;
                };
                let idle_time = now.duration_since(inner.last_activity);
                let max_age = pool_timeouts
                    .get(&inner.pool_name)
                    .copied()
                    .unwrap_or_else(|| Duration::from_secs(300));

                if idle_time > max_age && inner.is_active() {
                    warn!(
                        "Marking stale transaction {} for cleanup (idle: {:?}, created: {:?} ago)",
                        tx_id,
                        idle_time,
                        now.duration_since(inner.created_at)
                    );
                    stale.push((*tx_id, Arc::clone(tx_arc)));
                    false
                } else {
                    true
                }
            });

            stale
        };

        // Phase 2: rollback outside the map lock. If somebody grabbed the
        // entry lock meanwhile, dropping our Arc still guarantees rollback.
        let mut removed = 0;
        for (tx_id, tx_arc) in stale {
            removed += 1;
            if let Ok(mut inner) = tx_arc.try_lock() {
                if let Err(e) = inner.rollback().await {
                    warn!("Failed to rollback stale transaction {}: {}", tx_id, e);
                } else {
                    debug!("Successfully rolled back stale transaction {}", tx_id);
                }
            }
        }

        removed
    }
}

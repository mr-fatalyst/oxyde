//! Transaction registry for managing active transactions

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
    /// Track how many cleanup cycles a locked transaction has persisted
    locked_counts: Mutex<HashMap<u64, u32>>,
}

impl TransactionRegistry {
    pub fn new() -> Self {
        Self {
            transactions: RwLock::new(HashMap::new()),
            pool_settings: RwLock::new(HashMap::new()),
            locked_counts: Mutex::new(HashMap::new()),
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

        // Start cleanup task if not running (will be called from lib.rs which has static ref)
        // We don't call it here to avoid lifetime issues

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

    /// Rollback and remove all active transactions (for shutdown)
    pub async fn rollback_all(&self) -> usize {
        let to_rollback: Vec<(u64, Arc<Mutex<TransactionInner>>)> = {
            let mut guard = self.transactions.write().await;
            guard.drain().collect()
        };

        let mut count = 0;
        for (tx_id, tx_arc) in to_rollback {
            if let Ok(mut inner) = tx_arc.try_lock() {
                if inner.is_active() {
                    if let Err(e) = inner.rollback().await {
                        warn!(
                            "Failed to rollback transaction {} on shutdown: {}",
                            tx_id, e
                        );
                    } else {
                        debug!("Rolled back transaction {} on shutdown", tx_id);
                    }
                    count += 1;
                }
            } else {
                warn!(
                    "Could not acquire lock to rollback transaction {} on shutdown",
                    tx_id
                );
                count += 1;
            }
        }

        // Clear locked counts
        self.locked_counts.lock().await.clear();

        count
    }

    pub async fn cleanup_stale_transactions(&self) -> usize {
        const MAX_LOCKED_CYCLES: u32 = 5; // Force cleanup after 5 cleanup cycles

        // Get pool timeouts first
        let pool_timeouts: HashMap<String, Duration> = {
            let pool_settings = self.pool_settings.read().await;
            pool_settings
                .iter()
                .map(|(name, settings)| (name.clone(), settings.timeout))
                .collect()
        };

        // Phase 1: Identify transactions to cleanup (hold lock briefly)
        let to_cleanup: Vec<(u64, Arc<Mutex<TransactionInner>>)> = {
            let mut guard = self.transactions.write().await;
            let mut locked_counts = self.locked_counts.lock().await;
            let now = Instant::now();
            let mut to_cleanup = Vec::new();
            let mut locked_old_transactions = Vec::new();

            guard.retain(|tx_id, tx_arc| {
                let tx = tx_arc.try_lock();
                match tx {
                    Ok(inner) => {
                        // Transaction successfully locked - check last_activity against pool-specific timeout
                        let idle_time = now.duration_since(inner.last_activity);

                        // Get pool-specific timeout or use default
                        let max_age = pool_timeouts.get(&inner._pool_name)
                            .copied()
                            .unwrap_or_else(|| Duration::from_secs(300));

                        if idle_time > max_age && inner.is_active() {
                            warn!(
                                "Marking stale transaction {} for cleanup (idle: {:?}, created: {:?} ago)",
                                tx_id, idle_time, now.duration_since(inner.created_at)
                            );
                            locked_counts.remove(tx_id);
                            to_cleanup.push((*tx_id, Arc::clone(tx_arc)));
                            false // Remove from registry
                        } else {
                            // Transaction is alive and well, reset locked counter
                            locked_counts.remove(tx_id);
                            true
                        }
                    }
                    Err(_) => {
                        // Transaction is locked (actively executing query)
                        let strong_count = Arc::strong_count(tx_arc);
                        if strong_count == 1 {
                            // Only registry holds this Arc, but it's still locked - possible deadlock
                            let count = locked_counts.entry(*tx_id).or_insert(0);
                            *count += 1;

                            if *count >= MAX_LOCKED_CYCLES {
                                warn!(
                                    "Force marking transaction {} for cleanup after {} cycles with no active references",
                                    tx_id, count
                                );
                                locked_counts.remove(tx_id);
                                to_cleanup.push((*tx_id, Arc::clone(tx_arc)));
                                return false; // Remove from registry
                            }

                            warn!(
                                "Transaction {} is locked but has no active references. Locked cycle count: {}/{}",
                                tx_id, count, MAX_LOCKED_CYCLES
                            );
                            locked_old_transactions.push(*tx_id);
                        } else {
                            // Active transaction - reset counter if exists
                            locked_counts.remove(tx_id);
                        }
                        true // Keep locked transactions
                    }
                }
            });

            if !locked_old_transactions.is_empty() {
                debug!(
                    "Found {} locked transactions with no active references: {:?}",
                    locked_old_transactions.len(),
                    locked_old_transactions
                );
            }

            to_cleanup
        }; // Lock released here

        // Phase 2: Rollback transactions (no locks held)
        let mut removed = 0;
        for (tx_id, tx_arc) in to_cleanup {
            if let Ok(mut inner) = tx_arc.try_lock() {
                if let Err(e) = inner.rollback().await {
                    warn!("Failed to rollback stale transaction {}: {}", tx_id, e);
                } else {
                    debug!("Successfully rolled back stale transaction {}", tx_id);
                }
                removed += 1;
            } else {
                warn!("Could not acquire lock to rollback transaction {}", tx_id);
                removed += 1;
            }
        }

        removed
    }
}

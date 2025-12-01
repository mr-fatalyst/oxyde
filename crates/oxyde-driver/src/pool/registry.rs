//! Connection pool registry

use crate::error::{DriverError, Result};
use crate::pool::handle::PoolHandle;
use std::collections::HashMap;
use tokio::sync::RwLock;

pub(crate) struct ConnectionRegistry {
    pools: RwLock<HashMap<String, PoolHandle>>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
        }
    }

    pub async fn insert(&self, name: String, handle: PoolHandle) -> Result<()> {
        let mut guard = self.pools.write().await;
        if guard.contains_key(&name) {
            return Err(DriverError::PoolAlreadyExists(name));
        }
        guard.insert(name, handle);
        Ok(())
    }

    /// Insert or replace existing pool (closes old pool first)
    pub async fn insert_or_replace(&self, name: String, handle: PoolHandle) -> Option<PoolHandle> {
        let mut guard = self.pools.write().await;
        guard.insert(name, handle)
    }

    pub async fn get(&self, name: &str) -> Result<PoolHandle> {
        let guard = self.pools.read().await;
        guard
            .get(name)
            .cloned()
            .ok_or_else(|| DriverError::PoolNotFound(name.to_string()))
    }

    pub async fn remove(&self, name: &str) -> Result<PoolHandle> {
        let mut guard = self.pools.write().await;
        guard
            .remove(name)
            .ok_or_else(|| DriverError::PoolNotFound(name.to_string()))
    }

    pub async fn take_all(&self) -> Vec<(String, PoolHandle)> {
        let mut guard = self.pools.write().await;
        guard.drain().collect()
    }
}

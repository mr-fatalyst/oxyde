//! Pool configuration settings

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PoolSettings {
    pub max_connections: Option<u32>,
    pub min_connections: Option<u32>,
    pub idle_timeout: Option<Duration>,
    pub acquire_timeout: Option<Duration>,
    pub max_lifetime: Option<Duration>,
    pub test_before_acquire: Option<bool>,
    // Transaction cleanup settings
    pub transaction_timeout: Option<Duration>, // Max age before cleanup (default: 5 min)
    pub transaction_cleanup_interval: Option<Duration>, // How often to run cleanup (default: 60 sec)
    // SQLite-specific PRAGMA settings
    pub sqlite_journal_mode: Option<String>,
    pub sqlite_synchronous: Option<String>,
    pub sqlite_cache_size: Option<i32>,
    pub sqlite_busy_timeout: Option<i32>,
}

impl Default for PoolSettings {
    fn default() -> Self {
        Self {
            max_connections: Some(10),
            min_connections: Some(1),
            idle_timeout: None,
            acquire_timeout: None,
            max_lifetime: None,
            test_before_acquire: Some(false),
            transaction_timeout: Some(Duration::from_secs(300)), // 5 minutes
            transaction_cleanup_interval: Some(Duration::from_secs(60)), // 1 minute
            sqlite_journal_mode: None,
            sqlite_synchronous: None,
            sqlite_cache_size: None,
            sqlite_busy_timeout: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct PoolTimeoutSettings {
    pub(crate) timeout: Duration,
    pub(crate) cleanup_interval: Duration,
}

impl Default for PoolTimeoutSettings {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300),         // Default: 5 minutes
            cleanup_interval: Duration::from_secs(60), // Default: 1 minute
        }
    }
}

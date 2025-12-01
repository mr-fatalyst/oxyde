//! Error types for the driver

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DriverError {
    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Pool '{0}' already exists")]
    PoolAlreadyExists(String),

    #[error("Pool '{0}' not found")]
    PoolNotFound(String),

    #[error("Invalid pool settings: {0}")]
    InvalidPoolSettings(String),

    #[error("Query execution error: {0}")]
    ExecutionError(String),

    #[error("Transaction '{0}' not found")]
    TransactionNotFound(u64),

    #[error("Transaction '{0}' already completed")]
    TransactionClosed(u64),
}

pub type Result<T> = std::result::Result<T, DriverError>;

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

    /// A failed database call with the original `sqlx::Error` preserved,
    /// so constraint violations classify via [`DriverError::db_kind`]
    /// instead of message-text matching. Display mirrors the historical
    /// `ExecutionError` wording.
    #[error("Query execution error: {context}: {source}")]
    Db {
        context: String,
        #[source]
        source: sqlx::Error,
    },

    #[error("Transaction '{0}' not found")]
    TransactionNotFound(u64),

    #[error("Transaction '{0}' already completed")]
    TransactionClosed(u64),
}

/// Cross-dialect constraint classification (mirror of `sqlx::error::ErrorKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbErrorKind {
    UniqueViolation,
    ForeignKeyViolation,
    NotNullViolation,
    CheckViolation,
    Other,
}

impl DriverError {
    /// Build a [`DriverError::Db`] with human context, keeping the source.
    pub(crate) fn db(context: impl Into<String>, source: sqlx::Error) -> Self {
        Self::Db {
            context: context.into(),
            source,
        }
    }

    /// Constraint classification of the underlying database error, if any.
    #[must_use]
    pub fn db_kind(&self) -> Option<DbErrorKind> {
        let Self::Db { source, .. } = self else {
            return None;
        };
        let db = source.as_database_error()?;
        Some(match db.kind() {
            sqlx::error::ErrorKind::UniqueViolation => DbErrorKind::UniqueViolation,
            sqlx::error::ErrorKind::ForeignKeyViolation => DbErrorKind::ForeignKeyViolation,
            sqlx::error::ErrorKind::NotNullViolation => DbErrorKind::NotNullViolation,
            sqlx::error::ErrorKind::CheckViolation => DbErrorKind::CheckViolation,
            _ => DbErrorKind::Other,
        })
    }

    /// SQLSTATE (or dialect-native code) of the underlying error, if any.
    #[must_use]
    pub fn sqlstate(&self) -> Option<String> {
        let Self::Db { source, .. } = self else {
            return None;
        };
        Some(source.as_database_error()?.code()?.into_owned())
    }
}

pub type Result<T> = std::result::Result<T, DriverError>;

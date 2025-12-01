//! Transaction inner state

use crate::error::{DriverError, Result};
use crate::pool::{DatabaseBackend, DbPool};
use std::time::Instant;

pub(crate) enum DbConn {
    Postgres(sqlx::pool::PoolConnection<sqlx::Postgres>),
    MySql(sqlx::pool::PoolConnection<sqlx::MySql>),
    Sqlite(sqlx::pool::PoolConnection<sqlx::Sqlite>),
}

/// Execute the same code for all database backends.
/// Usage: `with_conn!(conn, |c| { sqlx::query("...").execute(c.as_mut()).await })`
macro_rules! with_conn {
    ($conn:expr, |$c:ident| $body:expr) => {
        match $conn {
            DbConn::Postgres($c) => $body,
            DbConn::MySql($c) => $body,
            DbConn::Sqlite($c) => $body,
        }
    };
}

pub(crate) use with_conn;

/// Acquire a connection from pool and begin a transaction.
/// Returns a DbConn with an active transaction.
pub(crate) async fn begin_on_pool(pool: &DbPool, backend: DatabaseBackend) -> Result<DbConn> {
    // MySQL uses START TRANSACTION, others use BEGIN
    let begin_sql = match backend {
        DatabaseBackend::MySql => "START TRANSACTION",
        _ => "BEGIN",
    };

    match pool {
        DbPool::Postgres(p) => {
            let mut conn = p
                .acquire()
                .await
                .map_err(|e| DriverError::ExecutionError(format!("Acquire failed: {}", e)))?;
            sqlx::query(begin_sql)
                .execute(conn.as_mut())
                .await
                .map_err(|e| DriverError::ExecutionError(format!("{} failed: {}", begin_sql, e)))?;
            Ok(DbConn::Postgres(conn))
        }
        DbPool::MySql(p) => {
            let mut conn = p
                .acquire()
                .await
                .map_err(|e| DriverError::ExecutionError(format!("Acquire failed: {}", e)))?;
            sqlx::query(begin_sql)
                .execute(conn.as_mut())
                .await
                .map_err(|e| DriverError::ExecutionError(format!("{} failed: {}", begin_sql, e)))?;
            Ok(DbConn::MySql(conn))
        }
        DbPool::Sqlite(p) => {
            let mut conn = p
                .acquire()
                .await
                .map_err(|e| DriverError::ExecutionError(format!("Acquire failed: {}", e)))?;
            sqlx::query(begin_sql)
                .execute(conn.as_mut())
                .await
                .map_err(|e| DriverError::ExecutionError(format!("{} failed: {}", begin_sql, e)))?;
            Ok(DbConn::Sqlite(conn))
        }
    }
}

pub(crate) enum TransactionState {
    Active,
    Committed,
    RolledBack,
}

pub(crate) struct TransactionInner {
    pub(crate) _pool_name: String,
    pub(crate) _backend: DatabaseBackend,
    pub(crate) conn: Option<DbConn>,
    pub(crate) state: TransactionState,
    pub(crate) created_at: Instant,
    pub(crate) last_activity: Instant,
}

impl TransactionInner {
    pub fn is_active(&self) -> bool {
        matches!(self.state, TransactionState::Active)
    }

    pub fn update_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Rollback transaction and mark as rolled back
    pub async fn rollback(&mut self) -> Result<()> {
        if !self.is_active() {
            return Ok(()); // Already committed or rolled back
        }

        if let Some(conn) = self.conn.as_mut() {
            with_conn!(conn, |c| {
                sqlx::query("ROLLBACK")
                    .execute(&mut **c)
                    .await
                    .map_err(|e| DriverError::ExecutionError(format!("ROLLBACK failed: {}", e)))
                    .map(|_| ())?
            });
        }

        self.state = TransactionState::RolledBack;
        Ok(())
    }
}

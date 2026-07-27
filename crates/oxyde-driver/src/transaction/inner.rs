//! Transaction inner state.
//!
//! Built on `sqlx::Transaction`: dropping an uncommitted transaction rolls
//! it back before the connection returns to the pool, so no error path can
//! leak an open transaction into pooled connections.

use sqlx::Executor;

use crate::error::{DriverError, Result};
use crate::pool::DbPool;
use std::time::Instant;

pub(crate) enum DbTx {
    Postgres(sqlx::Transaction<'static, sqlx::Postgres>),
    MySql(sqlx::Transaction<'static, sqlx::MySql>),
    Sqlite(sqlx::Transaction<'static, sqlx::Sqlite>),
}

impl DbTx {
    pub(crate) async fn commit(self) -> Result<()> {
        let result = match self {
            DbTx::Postgres(tx) => tx.commit().await,
            DbTx::MySql(tx) => tx.commit().await,
            DbTx::Sqlite(tx) => tx.commit().await,
        };
        result.map_err(|e| DriverError::db("COMMIT failed", e))
    }

    pub(crate) async fn rollback(self) -> Result<()> {
        let result = match self {
            DbTx::Postgres(tx) => tx.rollback().await,
            DbTx::MySql(tx) => tx.rollback().await,
            DbTx::Sqlite(tx) => tx.rollback().await,
        };
        result.map_err(|e| DriverError::db("ROLLBACK failed", e))
    }

    /// Execute a raw statement inside the transaction (savepoints).
    pub(crate) async fn execute_raw(&mut self, sql: &str) -> std::result::Result<(), sqlx::Error> {
        match self {
            DbTx::Postgres(tx) => tx.execute(sql).await.map(|_| ()),
            DbTx::MySql(tx) => tx.execute(sql).await.map(|_| ()),
            DbTx::Sqlite(tx) => tx.execute(sql).await.map(|_| ()),
        }
    }
}

/// Begin a transaction on a pooled connection (sqlx emits the dialect's BEGIN).
pub(crate) async fn begin_on_pool(pool: &DbPool) -> Result<DbTx> {
    let begin_err = |e: sqlx::Error| DriverError::db("BEGIN failed", e);
    match pool {
        DbPool::Postgres(p) => Ok(DbTx::Postgres(p.begin().await.map_err(begin_err)?)),
        DbPool::MySql(p) => Ok(DbTx::MySql(p.begin().await.map_err(begin_err)?)),
        DbPool::Sqlite(p) => Ok(DbTx::Sqlite(p.begin().await.map_err(begin_err)?)),
    }
}

pub(crate) struct TransactionInner {
    pub(crate) pool_name: String,
    /// `None` after commit/rollback. Dropping a `Some` rolls back via sqlx.
    pub(crate) tx: Option<DbTx>,
    pub(crate) created_at: Instant,
    pub(crate) last_activity: Instant,
}

impl TransactionInner {
    pub fn is_active(&self) -> bool {
        self.tx.is_some()
    }

    pub fn update_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Rollback explicitly; a no-op if already committed or rolled back.
    pub async fn rollback(&mut self) -> Result<()> {
        match self.tx.take() {
            Some(tx) => tx.rollback().await,
            None => Ok(()),
        }
    }
}

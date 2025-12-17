//! Pool handle wrapper

use sqlx::{mysql::MySqlPool, postgres::PgPool, sqlite::SqlitePool};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseBackend {
    Postgres,
    MySql,
    Sqlite,
}

/// Database pool enum holding connections for different backends.
/// Made public when pyo3 feature is enabled for direct conversion.
#[derive(Clone)]
#[cfg_attr(feature = "pyo3", derive())]
pub enum DbPool {
    Postgres(PgPool),
    MySql(MySqlPool),
    Sqlite(SqlitePool),
}

#[derive(Clone)]
pub struct PoolHandle {
    pub(crate) backend: DatabaseBackend,
    pub(crate) pool: DbPool,
}

impl PoolHandle {
    pub fn new(backend: DatabaseBackend, pool: DbPool) -> Self {
        Self { backend, pool }
    }

    pub async fn close(&self) {
        match &self.pool {
            DbPool::Postgres(pool) => pool.close().await,
            DbPool::MySql(pool) => pool.close().await,
            DbPool::Sqlite(pool) => pool.close().await,
        }
    }

    pub fn clone_pool(&self) -> DbPool {
        self.pool.clone()
    }
}

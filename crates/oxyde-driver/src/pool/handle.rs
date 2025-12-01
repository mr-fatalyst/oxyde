//! Pool handle wrapper

use crate::settings::PoolSettings;
use sqlx::{mysql::MySqlPool, postgres::PgPool, sqlite::SqlitePool};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseBackend {
    Postgres,
    MySql,
    Sqlite,
}

#[derive(Clone)]
pub(crate) enum DbPool {
    Postgres(PgPool),
    MySql(MySqlPool),
    Sqlite(SqlitePool),
}

#[derive(Clone)]
pub struct PoolHandle {
    pub(crate) backend: DatabaseBackend,
    pub(crate) pool: DbPool,
    #[allow(dead_code)]
    pub(crate) settings: PoolSettings,
}

impl PoolHandle {
    pub fn new(backend: DatabaseBackend, pool: DbPool, settings: PoolSettings) -> Self {
        Self {
            backend,
            pool,
            settings,
        }
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

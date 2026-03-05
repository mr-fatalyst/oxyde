//! Backend-agnostic query execution traits.
//!
//! This module provides traits that abstract over the three database backends
//! (PostgreSQL, MySQL, SQLite), reducing code duplication in query execution.

use async_trait::async_trait;
use sea_query::Value;
use std::collections::HashMap;

use crate::bind::{bind_mysql, bind_postgres, bind_sqlite};
use crate::convert::encoder::{encode_mutation_returning, encode_rows_columnar};
use crate::convert::mysql_enc::MySqlEncoder;
use crate::convert::postgres_enc::PgEncoder;
use crate::convert::sqlite_enc::SqliteEncoder;
use crate::convert::{convert_mysql_rows_typed, convert_pg_rows_typed, convert_sqlite_rows_typed};
use crate::error::{DriverError, Result};
use crate::pool::DbPool;
use crate::transaction::DbConn;

/// Format execution error message
fn exec_err(e: sqlx::Error) -> DriverError {
    DriverError::ExecutionError(format!("Query failed: {}", e))
}

fn stmt_err(e: sqlx::Error) -> DriverError {
    DriverError::ExecutionError(format!("Statement failed: {}", e))
}

/// Trait for executing queries on a database pool.
/// Uses `&self` because sqlx pools are internally reference-counted.
#[async_trait]
pub trait PoolExec {
    /// Execute a SELECT query and return rows as HashMaps
    async fn query(
        &self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<Vec<HashMap<String, serde_json::Value>>>;

    /// Execute a SELECT query and return pre-encoded msgpack bytes + row count.
    async fn query_columnar(
        &self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<(Vec<u8>, usize)>;

    /// Execute a mutation with RETURNING clause, return pre-encoded msgpack map.
    async fn query_mutation_returning(
        &self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<Vec<u8>>;

    /// Execute a statement (INSERT/UPDATE/DELETE) and return affected rows
    async fn execute(&self, sql: &str, params: &[Value]) -> Result<u64>;

    /// Backend name for logging/profiling
    fn backend_name(&self) -> &'static str;
}

/// Trait for executing queries on a database connection (in transaction).
/// Uses `&mut self` because sqlx connections require mutable access.
#[async_trait]
pub trait ConnExec {
    /// Execute a SELECT query and return rows as HashMaps
    async fn query(
        &mut self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<Vec<HashMap<String, serde_json::Value>>>;

    /// Execute a SELECT query and return pre-encoded msgpack bytes + row count.
    async fn query_columnar(
        &mut self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<(Vec<u8>, usize)>;

    /// Execute a mutation with RETURNING clause, return pre-encoded msgpack map.
    async fn query_mutation_returning(
        &mut self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<Vec<u8>>;

    /// Execute a statement and return affected rows
    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<u64>;
}

// =============================================================================
// DbPool implementation
// =============================================================================

#[async_trait]
impl PoolExec for DbPool {
    async fn query(
        &self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<Vec<HashMap<String, serde_json::Value>>> {
        match self {
            DbPool::Postgres(pool) => {
                let query = bind_postgres(sqlx::query(sql), params)?;
                let rows = query.fetch_all(pool).await.map_err(exec_err)?;
                Ok(convert_pg_rows_typed(rows, col_types))
            }
            DbPool::MySql(pool) => {
                let query = bind_mysql(sqlx::query(sql), params)?;
                let rows = query.fetch_all(pool).await.map_err(exec_err)?;
                Ok(convert_mysql_rows_typed(rows, col_types))
            }
            DbPool::Sqlite(pool) => {
                let query = bind_sqlite(sqlx::query(sql), params)?;
                let rows = query.fetch_all(pool).await.map_err(exec_err)?;
                Ok(convert_sqlite_rows_typed(rows, col_types))
            }
        }
    }

    async fn query_columnar(
        &self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<(Vec<u8>, usize)> {
        match self {
            DbPool::Postgres(pool) => {
                let query = bind_postgres(sqlx::query(sql), params)?;
                let rows = query.fetch_all(pool).await.map_err(exec_err)?;
                Ok(encode_rows_columnar::<PgEncoder>(&rows, col_types))
            }
            DbPool::MySql(pool) => {
                let query = bind_mysql(sqlx::query(sql), params)?;
                let rows = query.fetch_all(pool).await.map_err(exec_err)?;
                Ok(encode_rows_columnar::<MySqlEncoder>(&rows, col_types))
            }
            DbPool::Sqlite(pool) => {
                let query = bind_sqlite(sqlx::query(sql), params)?;
                let rows = query.fetch_all(pool).await.map_err(exec_err)?;
                Ok(encode_rows_columnar::<SqliteEncoder>(&rows, col_types))
            }
        }
    }

    async fn query_mutation_returning(
        &self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<Vec<u8>> {
        match self {
            DbPool::Postgres(pool) => {
                let query = bind_postgres(sqlx::query(sql), params)?;
                let rows = query.fetch_all(pool).await.map_err(exec_err)?;
                Ok(encode_mutation_returning::<PgEncoder>(&rows, col_types))
            }
            DbPool::MySql(pool) => {
                let query = bind_mysql(sqlx::query(sql), params)?;
                let rows = query.fetch_all(pool).await.map_err(exec_err)?;
                Ok(encode_mutation_returning::<MySqlEncoder>(&rows, col_types))
            }
            DbPool::Sqlite(pool) => {
                let query = bind_sqlite(sqlx::query(sql), params)?;
                let rows = query.fetch_all(pool).await.map_err(exec_err)?;
                Ok(encode_mutation_returning::<SqliteEncoder>(&rows, col_types))
            }
        }
    }

    async fn execute(&self, sql: &str, params: &[Value]) -> Result<u64> {
        match self {
            DbPool::Postgres(pool) => {
                let query = bind_postgres(sqlx::query(sql), params)?;
                let result = query.execute(pool).await.map_err(stmt_err)?;
                Ok(result.rows_affected())
            }
            DbPool::MySql(pool) => {
                let query = bind_mysql(sqlx::query(sql), params)?;
                let result = query.execute(pool).await.map_err(stmt_err)?;
                Ok(result.rows_affected())
            }
            DbPool::Sqlite(pool) => {
                let query = bind_sqlite(sqlx::query(sql), params)?;
                let result = query.execute(pool).await.map_err(stmt_err)?;
                Ok(result.rows_affected())
            }
        }
    }

    fn backend_name(&self) -> &'static str {
        match self {
            DbPool::Postgres(_) => "Postgres",
            DbPool::MySql(_) => "MySQL",
            DbPool::Sqlite(_) => "SQLite",
        }
    }
}

// =============================================================================
// DbConn implementation (for transactions)
// =============================================================================

#[async_trait]
impl ConnExec for DbConn {
    async fn query(
        &mut self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<Vec<HashMap<String, serde_json::Value>>> {
        match self {
            DbConn::Postgres(conn) => {
                let query = bind_postgres(sqlx::query(sql), params)?;
                let rows = query.fetch_all(conn.as_mut()).await.map_err(exec_err)?;
                Ok(convert_pg_rows_typed(rows, col_types))
            }
            DbConn::MySql(conn) => {
                let query = bind_mysql(sqlx::query(sql), params)?;
                let rows = query.fetch_all(conn.as_mut()).await.map_err(exec_err)?;
                Ok(convert_mysql_rows_typed(rows, col_types))
            }
            DbConn::Sqlite(conn) => {
                let query = bind_sqlite(sqlx::query(sql), params)?;
                let rows = query.fetch_all(conn.as_mut()).await.map_err(exec_err)?;
                Ok(convert_sqlite_rows_typed(rows, col_types))
            }
        }
    }

    async fn query_columnar(
        &mut self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<(Vec<u8>, usize)> {
        match self {
            DbConn::Postgres(conn) => {
                let query = bind_postgres(sqlx::query(sql), params)?;
                let rows = query.fetch_all(conn.as_mut()).await.map_err(exec_err)?;
                Ok(encode_rows_columnar::<PgEncoder>(&rows, col_types))
            }
            DbConn::MySql(conn) => {
                let query = bind_mysql(sqlx::query(sql), params)?;
                let rows = query.fetch_all(conn.as_mut()).await.map_err(exec_err)?;
                Ok(encode_rows_columnar::<MySqlEncoder>(&rows, col_types))
            }
            DbConn::Sqlite(conn) => {
                let query = bind_sqlite(sqlx::query(sql), params)?;
                let rows = query.fetch_all(conn.as_mut()).await.map_err(exec_err)?;
                Ok(encode_rows_columnar::<SqliteEncoder>(&rows, col_types))
            }
        }
    }

    async fn query_mutation_returning(
        &mut self,
        sql: &str,
        params: &[Value],
        col_types: Option<&HashMap<String, String>>,
    ) -> Result<Vec<u8>> {
        match self {
            DbConn::Postgres(conn) => {
                let query = bind_postgres(sqlx::query(sql), params)?;
                let rows = query.fetch_all(conn.as_mut()).await.map_err(exec_err)?;
                Ok(encode_mutation_returning::<PgEncoder>(&rows, col_types))
            }
            DbConn::MySql(conn) => {
                let query = bind_mysql(sqlx::query(sql), params)?;
                let rows = query.fetch_all(conn.as_mut()).await.map_err(exec_err)?;
                Ok(encode_mutation_returning::<MySqlEncoder>(&rows, col_types))
            }
            DbConn::Sqlite(conn) => {
                let query = bind_sqlite(sqlx::query(sql), params)?;
                let rows = query.fetch_all(conn.as_mut()).await.map_err(exec_err)?;
                Ok(encode_mutation_returning::<SqliteEncoder>(&rows, col_types))
            }
        }
    }

    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<u64> {
        match self {
            DbConn::Postgres(conn) => {
                let query = bind_postgres(sqlx::query(sql), params)?;
                let result = query.execute(conn.as_mut()).await.map_err(stmt_err)?;
                Ok(result.rows_affected())
            }
            DbConn::MySql(conn) => {
                let query = bind_mysql(sqlx::query(sql), params)?;
                let result = query.execute(conn.as_mut()).await.map_err(stmt_err)?;
                Ok(result.rows_affected())
            }
            DbConn::Sqlite(conn) => {
                let query = bind_sqlite(sqlx::query(sql), params)?;
                let result = query.execute(conn.as_mut()).await.map_err(stmt_err)?;
                Ok(result.rows_affected())
            }
        }
    }
}

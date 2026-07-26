//! EXPLAIN query functionality
//!
//! Uses a local query helper (not the main columnar path) because EXPLAIN
//! results are returned as `Vec<HashMap<String, serde_json::Value>>` for the
//! extract functions. This is a cold debug path — no hot-path allocation concern.

pub mod mysql;
pub mod postgres;
pub mod sqlite;

pub use mysql::{build_mysql_explain_sql, extract_mysql_json_plan};
pub use postgres::{
    build_postgres_explain_sql, extract_postgres_json_plan, extract_text_plan, ExplainFormat,
    ExplainOptions,
};
pub use sqlite::build_sqlite_explain_sql;

use std::collections::HashMap;

use sqlx::{Column, Row};

use crate::bind::{bind_mysql, bind_postgres, bind_sqlite};
use crate::error::{DriverError, Result};
use crate::pool::{DatabaseBackend, DbPool};
use crate::registry;

/// Convert a single sqlx row to HashMap<String, serde_json::Value>.
/// Tries i64 → f64 → bool → String (covers all EXPLAIN output types).
fn row_to_map<R: Row>(row: &R) -> HashMap<String, serde_json::Value>
where
    for<'r> i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> f64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> bool: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> serde_json::Value: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> &'r str: sqlx::ColumnIndex<R>,
    <<R as Row>::Database as sqlx::Database>::Column: Column,
{
    let mut map = HashMap::new();
    for col in row.columns() {
        let name = col.name();
        // Try JSON first (Postgres EXPLAIN FORMAT JSON returns jsonb)
        let val = if let Ok(v) = row.try_get::<serde_json::Value, _>(name) {
            v
        } else if let Ok(v) = row.try_get::<i64, _>(name) {
            serde_json::Value::Number(serde_json::Number::from(v))
        } else if let Ok(v) = row.try_get::<f64, _>(name) {
            serde_json::Number::from_f64(v)
                .map_or(serde_json::Value::Null, serde_json::Value::Number)
        } else if let Ok(v) = row.try_get::<bool, _>(name) {
            serde_json::Value::Bool(v)
        } else if let Ok(v) = row.try_get::<String, _>(name) {
            serde_json::Value::String(v)
        } else {
            serde_json::Value::Null
        };
        map.insert(name.to_string(), val);
    }
    map
}

/// Fetch EXPLAIN rows as Vec<HashMap> (cold debug path, uses pool directly).
async fn fetch_explain_rows(
    pool_name: &str,
    sql: &str,
    params: &[sea_query::Value],
) -> Result<Vec<HashMap<String, serde_json::Value>>> {
    let handle = registry().get(pool_name).await?;
    match handle.clone_pool() {
        DbPool::Postgres(pool) => {
            let query = bind_postgres(sqlx::query(sql), params)?;
            let rows = query
                .fetch_all(&pool)
                .await
                .map_err(|e| DriverError::ExecutionError(format!("EXPLAIN failed: {e}")))?;
            Ok(rows.iter().map(row_to_map).collect())
        }
        DbPool::MySql(pool) => {
            let query = bind_mysql(sqlx::query(sql), params)?;
            let rows = query
                .fetch_all(&pool)
                .await
                .map_err(|e| DriverError::ExecutionError(format!("EXPLAIN failed: {e}")))?;
            Ok(rows.iter().map(row_to_map).collect())
        }
        DbPool::Sqlite(pool) => {
            let query = bind_sqlite(sqlx::query(sql), params)?;
            let rows = query
                .fetch_all(&pool)
                .await
                .map_err(|e| DriverError::ExecutionError(format!("EXPLAIN failed: {e}")))?;
            Ok(rows.iter().map(row_to_map).collect())
        }
    }
}

pub fn rows_to_objects(rows: Vec<HashMap<String, serde_json::Value>>) -> serde_json::Value {
    let mut array = Vec::with_capacity(rows.len());
    for row in rows {
        let mut obj = serde_json::Map::new();
        for (key, value) in row {
            obj.insert(key, value);
        }
        array.push(serde_json::Value::Object(obj));
    }
    serde_json::Value::Array(array)
}

pub async fn explain_query(
    pool_name: &str,
    sql: &str,
    params: &[sea_query::Value],
    options: ExplainOptions,
) -> Result<serde_json::Value> {
    let backend = crate::pool::api::pool_backend(pool_name).await?;
    let explain_sql = match backend {
        DatabaseBackend::Postgres => build_postgres_explain_sql(sql, options),
        DatabaseBackend::MySql => build_mysql_explain_sql(sql, options),
        DatabaseBackend::Sqlite => build_sqlite_explain_sql(sql, options)?,
    };

    let rows = fetch_explain_rows(pool_name, &explain_sql, params).await?;

    let payload = match backend {
        DatabaseBackend::Postgres => match options.format {
            ExplainFormat::Json => extract_postgres_json_plan(rows),
            ExplainFormat::Text => extract_text_plan(rows, "QUERY PLAN"),
        },
        DatabaseBackend::MySql => match options.format {
            ExplainFormat::Json => extract_mysql_json_plan(rows),
            ExplainFormat::Text => rows_to_objects(rows),
        },
        DatabaseBackend::Sqlite => rows_to_objects(rows),
    };

    Ok(payload)
}

//! EXPLAIN query functionality

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

use crate::error::Result;
use crate::execute::query::execute_query;
use crate::pool::DatabaseBackend;

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
        DatabaseBackend::Postgres => build_postgres_explain_sql(sql, &options)?,
        DatabaseBackend::MySql => build_mysql_explain_sql(sql, &options)?,
        DatabaseBackend::Sqlite => build_sqlite_explain_sql(sql, &options)?,
    };

    let rows = execute_query(pool_name, &explain_sql, params, None).await?;

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

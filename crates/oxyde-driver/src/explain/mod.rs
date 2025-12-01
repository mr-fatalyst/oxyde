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

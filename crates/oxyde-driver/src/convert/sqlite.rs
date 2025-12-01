//! SQLite type conversion

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use sqlx::{sqlite::SqliteRow, Column, Row};
use std::collections::HashMap;

pub fn convert_sqlite_row(row: SqliteRow) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::new();

    for (i, column) in row.columns().iter().enumerate() {
        let name = Column::name(column).to_string();
        let type_info = Column::type_info(column);
        let type_name = type_info.to_string().to_uppercase();
        let value = decode_sqlite_cell(&row, i, &type_name);
        map.insert(name, value);
    }

    map
}

pub fn decode_sqlite_cell(row: &SqliteRow, idx: usize, type_name: &str) -> serde_json::Value {
    match type_name {
        "BOOL" | "BOOLEAN" => match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::Bool(v),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_sqlite(row, idx),
        },
        name if name.contains("INT") => match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::Number(serde_json::Number::from(v)),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_sqlite(row, idx),
        },
        name if name.contains("REAL") || name.contains("FLOAT") || name.contains("DOUBLE") => {
            match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(v)) => serde_json::Number::from_f64(v)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_sqlite(row, idx),
            }
        }
        // NUMERIC/DECIMAL: preserve precision by returning as string
        name if name.contains("NUMERIC") || name.contains("DECIMAL") => {
            match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(v)) => serde_json::Value::String(v),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_sqlite(row, idx),
            }
        }
        name if name.contains("BLOB") => match row.try_get::<Option<Vec<u8>>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(BASE64_STANDARD.encode(v)),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_sqlite(row, idx),
        },
        // SQLite dynamic typing: try INTEGER, REAL, TEXT, then NULL
        "NULL" => {
            if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(idx) {
                return serde_json::Value::Number(serde_json::Number::from(v));
            }
            if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(idx) {
                if let Some(num) = serde_json::Number::from_f64(v) {
                    return serde_json::Value::Number(num);
                }
            }
            if let Ok(Some(v)) = row.try_get::<Option<String>, _>(idx) {
                return serde_json::Value::String(v);
            }
            serde_json::Value::Null
        }
        _ => match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_sqlite(row, idx),
        },
    }
}

fn fallback_string_sqlite(row: &SqliteRow, idx: usize) -> serde_json::Value {
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(v)) => serde_json::Value::String(v),
        Ok(None) => serde_json::Value::Null,
        Err(_) => serde_json::Value::Null,
    }
}

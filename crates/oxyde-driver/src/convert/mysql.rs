//! MySQL type conversion

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use sqlx::{mysql::MySqlRow, Column, Row};
use std::collections::HashMap;

pub fn convert_mysql_row(row: MySqlRow) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::new();

    for (i, column) in row.columns().iter().enumerate() {
        let name = Column::name(column).to_string();
        let type_info = Column::type_info(column);
        let type_name = type_info.to_string().to_uppercase();
        let value = decode_mysql_cell(&row, i, &type_name);
        map.insert(name, value);
    }

    map
}

pub fn decode_mysql_cell(row: &MySqlRow, idx: usize, type_name: &str) -> serde_json::Value {
    match type_name {
        "BOOL" | "BOOLEAN" | "TINYINT(1)" | "BIT" => match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::Bool(v),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        },
        name if name.contains("INT") => match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::Number(serde_json::Number::from(v)),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        },
        name if name.contains("DOUBLE") || name.contains("FLOAT") || name.contains("REAL") => {
            match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(v)) => serde_json::Number::from_f64(v)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_mysql(row, idx),
            }
        }
        // DECIMAL: preserve precision by returning as string
        name if name.contains("DECIMAL") || name.contains("NUMERIC") => {
            match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(v)) => serde_json::Value::String(v),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_mysql(row, idx),
            }
        }
        "JSON" => match row.try_get::<Option<serde_json::Value>, _>(idx) {
            Ok(Some(v)) => v,
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        },
        name if name.contains("DATETIME") || name.contains("TIMESTAMP") => {
            match row.try_get::<Option<NaiveDateTime>, _>(idx) {
                Ok(Some(v)) => {
                    serde_json::Value::String(v.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
                }
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_mysql(row, idx),
            }
        }
        "DATE" => match row.try_get::<Option<NaiveDate>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v.format("%Y-%m-%d").to_string()),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        },
        "TIME" => match row.try_get::<Option<NaiveTime>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v.format("%H:%M:%S%.f").to_string()),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        },
        name if name.contains("BLOB") || name.contains("BINARY") => {
            match row.try_get::<Option<Vec<u8>>, _>(idx) {
                Ok(Some(v)) => serde_json::Value::String(BASE64_STANDARD.encode(v)),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_mysql(row, idx),
            }
        }
        _ => match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        },
    }
}

fn fallback_string_mysql(row: &MySqlRow, idx: usize) -> serde_json::Value {
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(v)) => serde_json::Value::String(v),
        Ok(None) => serde_json::Value::Null,
        Err(_) => serde_json::Value::Null,
    }
}

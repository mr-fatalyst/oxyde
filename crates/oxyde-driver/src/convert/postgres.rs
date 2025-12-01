//! PostgreSQL type conversion

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{postgres::PgRow, Column, Row};
use std::collections::HashMap;
use uuid::Uuid;

pub fn convert_pg_row(row: PgRow) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::new();

    for (i, column) in row.columns().iter().enumerate() {
        let name = Column::name(column).to_string();
        let type_info = Column::type_info(column);
        let type_name = type_info.to_string().to_uppercase();
        let value = decode_pg_cell(&row, i, &type_name);
        map.insert(name, value);
    }

    map
}

pub fn decode_pg_cell(row: &PgRow, idx: usize, type_name: &str) -> serde_json::Value {
    match type_name {
        "BOOL" | "BOOLEAN" => match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::Bool(v),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_pg(row, idx),
        },
        name if name.contains("INT") => match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::Number(serde_json::Number::from(v)),
            Ok(None) => serde_json::Value::Null,
            Err(_) => match row.try_get::<Option<i32>, _>(idx) {
                Ok(Some(v)) => serde_json::Value::Number(serde_json::Number::from(v as i64)),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_pg(row, idx),
            },
        },
        name if name.contains("FLOAT") || name.contains("DOUBLE") || name.contains("REAL") => {
            match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(v)) => serde_json::Number::from_f64(v)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_pg(row, idx),
            }
        }
        // NUMERIC/DECIMAL: preserve precision by returning as string
        name if name.contains("NUMERIC") || name.contains("DECIMAL") => {
            match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(v)) => serde_json::Value::String(v),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_pg(row, idx),
            }
        }
        "UUID" => match row.try_get::<Option<Uuid>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v.to_string()),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_pg(row, idx),
        },
        name if name == "JSON" || name == "JSONB" => {
            match row.try_get::<Option<serde_json::Value>, _>(idx) {
                Ok(Some(v)) => v,
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_pg(row, idx),
            }
        }
        name if name.contains("TIMESTAMPTZ") => {
            match row.try_get::<Option<DateTime<Utc>>, _>(idx) {
                Ok(Some(v)) => serde_json::Value::String(v.to_rfc3339()),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_pg(row, idx),
            }
        }
        name if name.contains("TIMESTAMP") => match row.try_get::<Option<NaiveDateTime>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v.format("%Y-%m-%dT%H:%M:%S%.f").to_string()),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_pg(row, idx),
        },
        "DATE" => match row.try_get::<Option<NaiveDate>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v.format("%Y-%m-%d").to_string()),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_pg(row, idx),
        },
        name if name == "TIME" || name == "TIMETZ" => {
            match row.try_get::<Option<NaiveTime>, _>(idx) {
                Ok(Some(v)) => serde_json::Value::String(v.format("%H:%M:%S%.f").to_string()),
                Ok(None) => serde_json::Value::Null,
                Err(_) => fallback_string_pg(row, idx),
            }
        }
        "BYTEA" => match row.try_get::<Option<Vec<u8>>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(BASE64_STANDARD.encode(v)),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_pg(row, idx),
        },
        _ => match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_pg(row, idx),
        },
    }
}

fn fallback_string_pg(row: &PgRow, idx: usize) -> serde_json::Value {
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(v)) => serde_json::Value::String(v),
        Ok(None) => serde_json::Value::Null,
        Err(_) => serde_json::Value::Null,
    }
}

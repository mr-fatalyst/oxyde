//! MySQL CellEncoder implementation.
//!
//! Encodes MySQL row cells directly to msgpack bytes, dispatched by
//! `ColumnTypeSpec`. Error-handling per arm is ported verbatim from the
//! legacy string-hint implementation.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use oxyde_codec::ColumnTypeSpec;
use rust_decimal::Decimal;
use sqlx::{mysql::MySqlRow, Row};

use super::encoder::*;

pub struct MySqlEncoder;

impl CellEncoder for MySqlEncoder {
    type Row = MySqlRow;

    fn try_encode_by_spec(
        buf: &mut Vec<u8>,
        row: &MySqlRow,
        idx: usize,
        spec: &ColumnTypeSpec,
    ) -> bool {
        match spec {
            ColumnTypeSpec::BigInteger => {
                match row.try_get::<Option<i64>, _>(idx) {
                    Ok(Some(v)) => write_i64(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            ColumnTypeSpec::Text | ColumnTypeSpec::String { .. } => {
                match row.try_get::<Option<String>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => write_nil(buf),
                }
                true
            }
            ColumnTypeSpec::Double => {
                match row.try_get::<Option<f64>, _>(idx) {
                    Ok(Some(v)) => write_f64(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            ColumnTypeSpec::Boolean => {
                match row.try_get::<Option<bool>, _>(idx) {
                    Ok(Some(v)) => write_bool(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            ColumnTypeSpec::Blob => {
                match row.try_get::<Option<Vec<u8>>, _>(idx) {
                    Ok(Some(v)) => write_bin(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            // MySQL stores both naive and aware datetimes as DATETIME(6)
            ColumnTypeSpec::DateTime | ColumnTypeSpec::DateTimeUtc => {
                match row.try_get::<Option<NaiveDateTime>, _>(idx) {
                    Ok(Some(v)) => {
                        write_str(buf, &v.format("%Y-%m-%dT%H:%M:%S%.f").to_string());
                    }
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            ColumnTypeSpec::Date => {
                match row.try_get::<Option<NaiveDate>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.format("%Y-%m-%d").to_string()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            ColumnTypeSpec::Time => {
                match row.try_get::<Option<NaiveTime>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.format("%H:%M:%S%.f").to_string()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            // UUID stored as CHAR(36) — read as text
            ColumnTypeSpec::Uuid => {
                match row.try_get::<Option<String>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => write_nil(buf),
                }
                true
            }
            ColumnTypeSpec::Decimal { .. } => {
                match row.try_get::<Option<Decimal>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.to_string()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            ColumnTypeSpec::Timedelta => {
                match row.try_get::<Option<i64>, _>(idx) {
                    Ok(Some(v)) => write_f64(buf, v as f64 / 1_000_000.0),
                    Ok(None) => write_nil(buf),
                    Err(_) => write_nil(buf),
                }
                true
            }
            // Arrays are stored as JSON in MySQL — element type is irrelevant
            ColumnTypeSpec::Json | ColumnTypeSpec::JsonBinary | ColumnTypeSpec::Array { .. } => {
                match row.try_get::<Option<serde_json::Value>, _>(idx) {
                    Ok(Some(v)) => write_json_value(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            ColumnTypeSpec::Unknown => false,
        }
    }

    fn encode_by_db_type(buf: &mut Vec<u8>, row: &MySqlRow, idx: usize, db_type: &str) {
        match db_type {
            "BOOL" | "BOOLEAN" | "TINYINT(1)" | "BIT" => {
                match row.try_get::<Option<bool>, _>(idx) {
                    Ok(Some(v)) => write_bool(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            name if name.contains("INT") => match row.try_get::<Option<i64>, _>(idx) {
                Ok(Some(v)) => write_i64(buf, v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            name if name.contains("DOUBLE") || name.contains("FLOAT") || name.contains("REAL") => {
                match row.try_get::<Option<f64>, _>(idx) {
                    Ok(Some(v)) => write_f64(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            name if name.contains("DECIMAL") || name.contains("NUMERIC") => {
                match row.try_get::<Option<Decimal>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.to_string()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            "JSON" => match row.try_get::<Option<serde_json::Value>, _>(idx) {
                Ok(Some(v)) => write_json_value(buf, &v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            name if name.contains("DATETIME") || name.contains("TIMESTAMP") => {
                match row.try_get::<Option<NaiveDateTime>, _>(idx) {
                    Ok(Some(v)) => {
                        write_str(buf, &v.format("%Y-%m-%dT%H:%M:%S%.f").to_string());
                    }
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            "DATE" => match row.try_get::<Option<NaiveDate>, _>(idx) {
                Ok(Some(v)) => write_str(buf, &v.format("%Y-%m-%d").to_string()),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            "TIME" => match row.try_get::<Option<NaiveTime>, _>(idx) {
                Ok(Some(v)) => write_str(buf, &v.format("%H:%M:%S%.f").to_string()),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            name if name.contains("BLOB") || name.contains("BINARY") => {
                match row.try_get::<Option<Vec<u8>>, _>(idx) {
                    Ok(Some(v)) => write_bin(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            _ => match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(v)) => write_str(buf, &v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
        }
    }
}

fn fallback_str(buf: &mut Vec<u8>, row: &MySqlRow, idx: usize) {
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(v)) => write_str(buf, &v),
        Ok(None) | Err(_) => write_nil(buf),
    }
}

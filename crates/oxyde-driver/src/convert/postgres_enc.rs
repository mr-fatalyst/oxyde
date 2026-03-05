//! PostgreSQL CellEncoder implementation.
//!
//! Encodes PostgreSQL row cells directly to msgpack bytes.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{postgres::PgRow, Row};
use uuid::Uuid;

use super::encoder::*;

pub struct PgEncoder;

impl CellEncoder for PgEncoder {
    type Row = PgRow;

    fn try_encode_by_ir_type(buf: &mut Vec<u8>, row: &PgRow, idx: usize, ir_type: &str) -> bool {
        match ir_type {
            "int" => {
                match row.try_get::<Option<i64>, _>(idx) {
                    Ok(Some(v)) => write_i64(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => match row.try_get::<Option<i32>, _>(idx) {
                        Ok(Some(v)) => write_i32_as_i64(buf, v),
                        Ok(None) => write_nil(buf),
                        Err(_) => fallback_str(buf, row, idx),
                    },
                }
                true
            }
            "str" => {
                match row.try_get::<Option<String>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => write_nil(buf),
                }
                true
            }
            "float" => {
                match row.try_get::<Option<f64>, _>(idx) {
                    Ok(Some(v)) => write_f64(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            "bool" => {
                match row.try_get::<Option<bool>, _>(idx) {
                    Ok(Some(v)) => write_bool(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            "bytes" => {
                match row.try_get::<Option<Vec<u8>>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &BASE64_STANDARD.encode(v)),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            "datetime" => {
                match row.try_get::<Option<DateTime<Utc>>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.to_rfc3339()),
                    Ok(None) => write_nil(buf),
                    Err(_) => match row.try_get::<Option<NaiveDateTime>, _>(idx) {
                        Ok(Some(v)) => {
                            write_str(buf, &v.format("%Y-%m-%dT%H:%M:%S%.f").to_string());
                        }
                        Ok(None) => write_nil(buf),
                        Err(_) => fallback_str(buf, row, idx),
                    },
                }
                true
            }
            "date" => {
                match row.try_get::<Option<NaiveDate>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.format("%Y-%m-%d").to_string()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            "time" => {
                match row.try_get::<Option<NaiveTime>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.format("%H:%M:%S%.f").to_string()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            "uuid" => {
                match row.try_get::<Option<Uuid>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.to_string()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            "decimal" | "timedelta" => {
                match row.try_get::<Option<String>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => write_nil(buf),
                }
                true
            }
            "json" => {
                match row.try_get::<Option<serde_json::Value>, _>(idx) {
                    Ok(Some(v)) => write_json_value(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            _ => false,
        }
    }

    fn encode_by_db_type(buf: &mut Vec<u8>, row: &PgRow, idx: usize, db_type: &str) {
        match db_type {
            "BOOL" | "BOOLEAN" => match row.try_get::<Option<bool>, _>(idx) {
                Ok(Some(v)) => write_bool(buf, v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            name if name.contains("INT") => match row.try_get::<Option<i64>, _>(idx) {
                Ok(Some(v)) => write_i64(buf, v),
                Ok(None) => write_nil(buf),
                Err(_) => match row.try_get::<Option<i32>, _>(idx) {
                    Ok(Some(v)) => write_i32_as_i64(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                },
            },
            name if name.contains("FLOAT") || name.contains("DOUBLE") || name.contains("REAL") => {
                match row.try_get::<Option<f64>, _>(idx) {
                    Ok(Some(v)) => write_f64(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            name if name.contains("NUMERIC") || name.contains("DECIMAL") => {
                match row.try_get::<Option<String>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            "UUID" => match row.try_get::<Option<Uuid>, _>(idx) {
                Ok(Some(v)) => write_str(buf, &v.to_string()),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            name if name == "JSON" || name == "JSONB" => {
                match row.try_get::<Option<serde_json::Value>, _>(idx) {
                    Ok(Some(v)) => write_json_value(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            name if name.contains("TIMESTAMPTZ") => {
                match row.try_get::<Option<DateTime<Utc>>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.to_rfc3339()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            name if name.contains("TIMESTAMP") => {
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
            name if name == "TIME" || name == "TIMETZ" => {
                match row.try_get::<Option<NaiveTime>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.format("%H:%M:%S%.f").to_string()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
            }
            "BYTEA" => match row.try_get::<Option<Vec<u8>>, _>(idx) {
                Ok(Some(v)) => write_str(buf, &BASE64_STANDARD.encode(v)),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            _ => match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(v)) => write_str(buf, &v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
        }
    }
}

fn fallback_str(buf: &mut Vec<u8>, row: &PgRow, idx: usize) {
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(v)) => write_str(buf, &v),
        Ok(None) | Err(_) => write_nil(buf),
    }
}

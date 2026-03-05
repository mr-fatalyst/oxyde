//! SQLite CellEncoder implementation.
//!
//! Encodes SQLite row cells directly to msgpack bytes.

use sqlx::{sqlite::SqliteRow, Row};

use super::encoder::*;

pub struct SqliteEncoder;

impl CellEncoder for SqliteEncoder {
    type Row = SqliteRow;

    fn try_encode_by_ir_type(
        buf: &mut Vec<u8>,
        row: &SqliteRow,
        idx: usize,
        ir_type: &str,
    ) -> bool {
        match ir_type {
            "int" => {
                match row.try_get::<Option<i64>, _>(idx) {
                    Ok(Some(v)) => write_i64(buf, v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
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
                    Ok(Some(v)) => write_bin(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            // datetime, date, time, timedelta, decimal, uuid — stored as TEXT in SQLite
            "datetime" | "date" | "time" | "timedelta" | "decimal" | "uuid" => {
                match row.try_get::<Option<String>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => write_nil(buf),
                }
                true
            }
            // JSON — stored as TEXT, parse to preserve structure
            "json" => {
                match row.try_get::<Option<String>, _>(idx) {
                    Ok(Some(v)) => {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&v) {
                            write_json_value(buf, &parsed);
                        } else {
                            write_str(buf, &v);
                        }
                    }
                    Ok(None) => write_nil(buf),
                    Err(_) => write_nil(buf),
                }
                true
            }
            _ => false,
        }
    }

    fn encode_by_db_type(buf: &mut Vec<u8>, row: &SqliteRow, idx: usize, db_type: &str) {
        match db_type {
            "BOOL" | "BOOLEAN" => match row.try_get::<Option<bool>, _>(idx) {
                Ok(Some(v)) => write_bool(buf, v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            name if name.contains("INT") => match row.try_get::<Option<i64>, _>(idx) {
                Ok(Some(v)) => write_i64(buf, v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            name if name.contains("REAL") || name.contains("FLOAT") || name.contains("DOUBLE") => {
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
            name if name.contains("BLOB") => match row.try_get::<Option<Vec<u8>>, _>(idx) {
                Ok(Some(v)) => write_bin(buf, &v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
            // SQLite dynamic typing: try INTEGER, REAL, TEXT, then NULL
            "NULL" => {
                if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(idx) {
                    write_i64(buf, v);
                    return;
                }
                if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(idx) {
                    write_f64(buf, v);
                    return;
                }
                if let Ok(Some(v)) = row.try_get::<Option<String>, _>(idx) {
                    write_str(buf, &v);
                    return;
                }
                write_nil(buf);
            }
            _ => match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(v)) => write_str(buf, &v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
        }
    }
}

fn fallback_str(buf: &mut Vec<u8>, row: &SqliteRow, idx: usize) {
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(v)) => write_str(buf, &v),
        Ok(None) | Err(_) => write_nil(buf),
    }
}

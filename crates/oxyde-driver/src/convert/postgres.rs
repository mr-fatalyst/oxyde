//! PostgreSQL CellEncoder implementation.
//!
//! Encodes PostgreSQL row cells directly to msgpack bytes.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
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
                    Ok(Some(v)) => write_bin(buf, &v),
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
            "decimal" => {
                match row.try_get::<Option<String>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => write_nil(buf),
                }
                true
            }
            "timedelta" => {
                match row.try_get::<Option<i64>, _>(idx) {
                    Ok(Some(v)) => write_i64(buf, v),
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
            ir_type if ir_type.ends_with("[]") => {
                encode_pg_array(buf, row, idx, &ir_type[..ir_type.len() - 2]);
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
                Ok(Some(v)) => write_bin(buf, &v),
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

/// Encode a native PostgreSQL array column to msgpack array.
/// Normalizes inner type to lowercase for case-insensitive matching
/// (handles both IR names like "int" and SQL names like "BIGINT").
/// Uses `Vec<Option<T>>` to correctly handle NULL elements within arrays.
fn encode_pg_array(buf: &mut Vec<u8>, row: &PgRow, idx: usize, inner_raw: &str) {
    let inner = inner_raw.to_ascii_lowercase();
    match inner.as_str() {
        "int" | "integer" | "bigint" | "smallint" | "tinyint" | "serial" | "bigserial"
        | "smallserial" | "int2" | "int4" | "int8" | "timedelta" | "interval" => {
            encode_pg_array_opt::<i64>(buf, row, idx, write_i64);
        }
        "float" | "double" | "real" | "float4" | "float8" | "double precision" => {
            encode_pg_array_opt::<f64>(buf, row, idx, write_f64);
        }
        "bool" | "boolean" => {
            encode_pg_array_opt::<bool>(buf, row, idx, write_bool);
        }
        "str" | "text" | "varchar" | "char" => {
            encode_pg_array_ref::<String>(buf, row, idx, |b, v| write_str(b, v));
        }
        "uuid" => {
            encode_pg_array_ref::<Uuid>(buf, row, idx, |b, v| {
                write_str(b, &v.to_string());
            });
        }
        "decimal" | "numeric" => {
            encode_pg_array_ref::<Decimal>(buf, row, idx, |b, v| {
                write_str(b, &v.to_string());
            });
        }
        "datetime" | "timestamp" => {
            encode_pg_array_ref::<NaiveDateTime>(buf, row, idx, |b, v| {
                write_str(b, &v.format("%Y-%m-%dT%H:%M:%S%.f").to_string());
            });
        }
        "timestamptz" => {
            encode_pg_array_ref::<DateTime<Utc>>(buf, row, idx, |b, v| {
                write_str(b, &v.to_rfc3339());
            });
        }
        "date" => {
            encode_pg_array_ref::<NaiveDate>(buf, row, idx, |b, v| {
                write_str(b, &v.format("%Y-%m-%d").to_string());
            });
        }
        "time" | "timetz" => {
            encode_pg_array_ref::<NaiveTime>(buf, row, idx, |b, v| {
                write_str(b, &v.format("%H:%M:%S%.f").to_string());
            });
        }
        "json" | "jsonb" => {
            encode_pg_array_ref::<serde_json::Value>(buf, row, idx, write_json_value);
        }
        // Unknown inner type — try as JSON array fallback
        _ => match row.try_get::<Option<serde_json::Value>, _>(idx) {
            Ok(Some(v)) => write_json_value(buf, &v),
            Ok(None) => write_nil(buf),
            Err(_) => fallback_str(buf, row, idx),
        },
    }
}

/// Helper: encode `Vec<Option<T>>` where T is Copy (i64, f64, bool).
/// Handles NULL elements within arrays.
fn encode_pg_array_opt<T>(
    buf: &mut Vec<u8>,
    row: &PgRow,
    idx: usize,
    write_fn: impl Fn(&mut Vec<u8>, T),
) where
    T: sqlx::Type<sqlx::Postgres> + for<'r> sqlx::Decode<'r, sqlx::Postgres> + Copy,
    Vec<Option<T>>: sqlx::Type<sqlx::Postgres> + for<'r> sqlx::Decode<'r, sqlx::Postgres>,
{
    match row.try_get::<Option<Vec<Option<T>>>, _>(idx) {
        Ok(Some(v)) => {
            write_array_len(buf, v.len() as u32);
            for item in &v {
                match item {
                    Some(val) => write_fn(buf, *val),
                    None => write_nil(buf),
                }
            }
        }
        Ok(None) => write_nil(buf),
        Err(_) => write_nil(buf),
    }
}

/// Helper: encode `Vec<Option<T>>` where T is not Copy (String, Uuid, etc.).
/// Handles NULL elements within arrays.
fn encode_pg_array_ref<T>(
    buf: &mut Vec<u8>,
    row: &PgRow,
    idx: usize,
    write_fn: impl Fn(&mut Vec<u8>, &T),
) where
    T: sqlx::Type<sqlx::Postgres> + for<'r> sqlx::Decode<'r, sqlx::Postgres>,
    Vec<Option<T>>: sqlx::Type<sqlx::Postgres> + for<'r> sqlx::Decode<'r, sqlx::Postgres>,
{
    match row.try_get::<Option<Vec<Option<T>>>, _>(idx) {
        Ok(Some(v)) => {
            write_array_len(buf, v.len() as u32);
            for item in &v {
                match item {
                    Some(val) => write_fn(buf, val),
                    None => write_nil(buf),
                }
            }
        }
        Ok(None) => write_nil(buf),
        Err(_) => write_nil(buf),
    }
}

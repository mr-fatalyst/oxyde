//! PostgreSQL CellEncoder implementation.
//!
//! Encodes PostgreSQL row cells directly to msgpack bytes, dispatched by
//! `ColumnTypeSpec`. Error-handling per arm (nil vs string-fallback) is
//! ported verbatim from the legacy string-hint implementation.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use oxyde_codec::ColumnTypeSpec;
use rust_decimal::Decimal;
use sqlx::{postgres::PgRow, Row};
use uuid::Uuid;

use super::encoder::*;

pub struct PgEncoder;

impl CellEncoder for PgEncoder {
    type Row = PgRow;

    fn try_encode_by_spec(
        buf: &mut Vec<u8>,
        row: &PgRow,
        idx: usize,
        spec: &ColumnTypeSpec,
    ) -> bool {
        match spec {
            ColumnTypeSpec::BigInteger => {
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
            ColumnTypeSpec::DateTime => {
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
            ColumnTypeSpec::DateTimeUtc => {
                match row.try_get::<Option<DateTime<Utc>>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.to_rfc3339()),
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
            ColumnTypeSpec::Uuid => {
                match row.try_get::<Option<Uuid>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.to_string()),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
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
            ColumnTypeSpec::Json | ColumnTypeSpec::JsonBinary => {
                match row.try_get::<Option<serde_json::Value>, _>(idx) {
                    Ok(Some(v)) => write_json_value(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            ColumnTypeSpec::Array { item } => {
                encode_pg_array(buf, row, idx, item);
                true
            }
            ColumnTypeSpec::Unknown => false,
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
                match row.try_get::<Option<Decimal>, _>(idx) {
                    Ok(Some(v)) => write_str(buf, &v.to_string()),
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

/// Encode a native PostgreSQL array column to a msgpack array, dispatched by
/// the element spec. Uses `Vec<Option<T>>` to handle NULL elements.
///
/// Legacy note: timedelta/interval array elements were always read as plain
/// i64 (BigInt group), unlike scalar timedelta (µs → f64) — preserved as is.
fn encode_pg_array(buf: &mut Vec<u8>, row: &PgRow, idx: usize, item: &ColumnTypeSpec) {
    match item {
        ColumnTypeSpec::BigInteger | ColumnTypeSpec::Timedelta => {
            // Try i64 first (BIGINT[]), fall back to i32 (INTEGER[]/SMALLINT[])
            if !try_encode_pg_array_opt::<i64>(buf, row, idx, write_i64) {
                encode_pg_array_opt::<i32>(buf, row, idx, |b, v| write_i64(b, i64::from(v)));
            }
        }
        ColumnTypeSpec::Double => {
            encode_pg_array_opt::<f64>(buf, row, idx, write_f64);
        }
        ColumnTypeSpec::Boolean => {
            encode_pg_array_opt::<bool>(buf, row, idx, write_bool);
        }
        ColumnTypeSpec::Text | ColumnTypeSpec::String { .. } => {
            encode_pg_array_ref::<String>(buf, row, idx, |b, v| write_str(b, v));
        }
        ColumnTypeSpec::Uuid => {
            encode_pg_array_ref::<Uuid>(buf, row, idx, |b, v| {
                write_str(b, &v.to_string());
            });
        }
        ColumnTypeSpec::Decimal { .. } => {
            encode_pg_array_ref::<Decimal>(buf, row, idx, |b, v| {
                write_str(b, &v.to_string());
            });
        }
        ColumnTypeSpec::DateTime => {
            encode_pg_array_ref::<NaiveDateTime>(buf, row, idx, |b, v| {
                write_str(b, &v.format("%Y-%m-%dT%H:%M:%S%.f").to_string());
            });
        }
        ColumnTypeSpec::DateTimeUtc => {
            encode_pg_array_ref::<DateTime<Utc>>(buf, row, idx, |b, v| {
                write_str(b, &v.to_rfc3339());
            });
        }
        ColumnTypeSpec::Date => {
            encode_pg_array_ref::<NaiveDate>(buf, row, idx, |b, v| {
                write_str(b, &v.format("%Y-%m-%d").to_string());
            });
        }
        ColumnTypeSpec::Time => {
            encode_pg_array_ref::<NaiveTime>(buf, row, idx, |b, v| {
                write_str(b, &v.format("%H:%M:%S%.f").to_string());
            });
        }
        ColumnTypeSpec::Json | ColumnTypeSpec::JsonBinary => {
            encode_pg_array_ref::<serde_json::Value>(buf, row, idx, write_json_value);
        }
        // Unknown / nested / bytea element types — JSON array fallback.
        // (Legacy had no bytea[] arm either; preserved as is.)
        ColumnTypeSpec::Unknown | ColumnTypeSpec::Array { .. } | ColumnTypeSpec::Blob => {
            match row.try_get::<Option<serde_json::Value>, _>(idx) {
                Ok(Some(v)) => write_json_value(buf, &v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            }
        }
    }
}

/// Try to encode `Vec<Option<T>>` where T is Copy. Returns false on type mismatch.
fn try_encode_pg_array_opt<T>(
    buf: &mut Vec<u8>,
    row: &PgRow,
    idx: usize,
    write_fn: impl Fn(&mut Vec<u8>, T),
) -> bool
where
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
            true
        }
        Ok(None) => {
            write_nil(buf);
            true
        }
        Err(_) => false,
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

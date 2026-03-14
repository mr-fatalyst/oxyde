//! PostgreSQL CellEncoder implementation.
//!
//! Encodes PostgreSQL row cells directly to msgpack bytes.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{postgres::PgRow, Row};
use uuid::Uuid;

use super::encoder::*;

pub struct PgEncoder;

impl CellEncoder for PgEncoder {
    type Row = PgRow;

    fn try_encode_by_ir_type(buf: &mut Vec<u8>, row: &PgRow, idx: usize, ir_type: &str) -> bool {
        // Normalize: Python may send uppercase DB type names (e.g., "TIMESTAMPTZ", "UUID")
        // when the field has an explicit db_type, instead of lowercase IR names.
        let ir_upper = ir_type.to_uppercase();
        let normalized = match ir_upper.as_str() {
            "TIMESTAMPTZ" | "TIMESTAMP" => "datetime",
            "UUID" => "uuid",
            "JSON" | "JSONB" => "json",
            "TEXT" | "VARCHAR" | "CHAR" => "str",
            "BOOLEAN" => "bool",
            "BIGINT" | "INTEGER" | "SMALLINT" | "INT2" | "INT4" | "INT8" => "int",
            "FLOAT4" | "FLOAT8" | "DOUBLE PRECISION" | "REAL" => "float",
            "BYTEA" => "bytes",
            "DATE" => "date",
            "TIME" | "TIMETZ" => "time",
            "NUMERIC" | "DECIMAL" => "decimal",
            _ => ir_type,
        };
        match normalized {
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
            "json" | "jsonb" => {
                match row.try_get::<Option<serde_json::Value>, _>(idx) {
                    Ok(Some(v)) => write_json_value(buf, &v),
                    Ok(None) => write_nil(buf),
                    Err(_) => fallback_str(buf, row, idx),
                }
                true
            }
            ir if ir.ends_with("[]") => {
                let base = &ir[..ir.len() - 2];
                encode_pg_array(buf, row, idx, base);
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
            // PostgreSQL array types (e.g. _TEXT, _UUID, _INT4, TEXT[], UUID[])
            name if name.ends_with("[]") || name.starts_with('_') => {
                let base = db_type_to_ir_base(name);
                encode_pg_array(buf, row, idx, base);
            }
            _ => match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(v)) => write_str(buf, &v),
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
        }
    }
}

/// Map a PostgreSQL array DB type name to an IR base type.
/// Handles both `_TEXT` (internal PG name) and `TEXT[]` formats.
fn db_type_to_ir_base(name: &str) -> &str {
    // Strip [] suffix or _ prefix to get the base type
    let base = if let Some(b) = name.strip_suffix("[]") {
        b
    } else if let Some(b) = name.strip_prefix('_') {
        b
    } else {
        name
    };
    match base.to_uppercase().as_str() {
        "TEXT" | "VARCHAR" | "CHAR" | "BPCHAR" | "NAME" => "str",
        "INT2" | "SMALLINT" | "INT4" | "INTEGER" | "INT" | "INT8" | "BIGINT" => "int",
        "FLOAT4" | "REAL" | "FLOAT8" | "DOUBLE PRECISION" => "float",
        "BOOL" | "BOOLEAN" => "bool",
        "UUID" => "uuid",
        "JSON" | "JSONB" => "json",
        "TIMESTAMPTZ" => "datetime",
        "TIMESTAMP" => "datetime",
        "DATE" => "date",
        "TIME" | "TIMETZ" => "time",
        _ => "str", // fallback
    }
}

/// Encode a PostgreSQL array column to msgpack array based on the base IR type.
fn encode_pg_array(buf: &mut Vec<u8>, row: &PgRow, idx: usize, base_type: &str) {
    // Normalize uppercase DB type names to lowercase IR names
    let base_upper = base_type.to_uppercase();
    let normalized_base = match base_upper.as_str() {
        "TEXT" | "VARCHAR" | "CHAR" | "BPCHAR" | "NAME" => "str",
        "INT2" | "SMALLINT" | "INT4" | "INTEGER" | "INT" | "INT8" | "BIGINT" => "int",
        "FLOAT4" | "REAL" | "FLOAT8" | "DOUBLE PRECISION" => "float",
        "BOOL" | "BOOLEAN" => "bool",
        "UUID" => "uuid",
        "JSON" | "JSONB" => "json",
        "TIMESTAMPTZ" | "TIMESTAMP" => "datetime",
        "DATE" => "date",
        "TIME" | "TIMETZ" => "time",
        _ => base_type,
    };
    match normalized_base {
        "str" => match row.try_get::<Option<Vec<String>>, _>(idx) {
            Ok(Some(arr)) => {
                rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                for v in &arr {
                    write_str(buf, v);
                }
            }
            Ok(None) => write_nil(buf),
            Err(_) => fallback_str(buf, row, idx),
        },
        "int" => match row.try_get::<Option<Vec<i64>>, _>(idx) {
            Ok(Some(arr)) => {
                rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                for v in &arr {
                    write_i64(buf, *v);
                }
            }
            Ok(None) => write_nil(buf),
            Err(_) => match row.try_get::<Option<Vec<i32>>, _>(idx) {
                Ok(Some(arr)) => {
                    rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                    for v in &arr {
                        write_i32_as_i64(buf, *v);
                    }
                }
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
        },
        "float" => match row.try_get::<Option<Vec<f64>>, _>(idx) {
            Ok(Some(arr)) => {
                rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                for v in &arr {
                    write_f64(buf, *v);
                }
            }
            Ok(None) => write_nil(buf),
            Err(_) => fallback_str(buf, row, idx),
        },
        "bool" => match row.try_get::<Option<Vec<bool>>, _>(idx) {
            Ok(Some(arr)) => {
                rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                for v in &arr {
                    write_bool(buf, *v);
                }
            }
            Ok(None) => write_nil(buf),
            Err(_) => fallback_str(buf, row, idx),
        },
        "uuid" => match row.try_get::<Option<Vec<Uuid>>, _>(idx) {
            Ok(Some(arr)) => {
                rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                for v in &arr {
                    write_str(buf, &v.to_string());
                }
            }
            Ok(None) => write_nil(buf),
            Err(_) => fallback_str(buf, row, idx),
        },
        "json" | "jsonb" => match row.try_get::<Option<Vec<serde_json::Value>>, _>(idx) {
            Ok(Some(arr)) => {
                rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                for v in &arr {
                    write_json_value(buf, v);
                }
            }
            Ok(None) => write_nil(buf),
            Err(_) => fallback_str(buf, row, idx),
        },
        "datetime" => match row.try_get::<Option<Vec<DateTime<Utc>>>, _>(idx) {
            Ok(Some(arr)) => {
                rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                for v in &arr {
                    write_str(buf, &v.to_rfc3339());
                }
            }
            Ok(None) => write_nil(buf),
            Err(_) => match row.try_get::<Option<Vec<NaiveDateTime>>, _>(idx) {
                Ok(Some(arr)) => {
                    rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                    for v in &arr {
                        write_str(buf, &v.format("%Y-%m-%dT%H:%M:%S%.f").to_string());
                    }
                }
                Ok(None) => write_nil(buf),
                Err(_) => fallback_str(buf, row, idx),
            },
        },
        // Fallback: try as string array
        _ => match row.try_get::<Option<Vec<String>>, _>(idx) {
            Ok(Some(arr)) => {
                rmp::encode::write_array_len(buf, arr.len() as u32).ok();
                for v in &arr {
                    write_str(buf, v);
                }
            }
            Ok(None) => write_nil(buf),
            Err(_) => fallback_str(buf, row, idx),
        },
    }
}

fn fallback_str(buf: &mut Vec<u8>, row: &PgRow, idx: usize) {
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(v)) => write_str(buf, &v),
        Ok(None) | Err(_) => write_nil(buf),
    }
}

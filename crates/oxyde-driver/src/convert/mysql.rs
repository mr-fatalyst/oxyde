//! MySQL type conversion
//!
//! Converts MySQL rows to HashMap<String, serde_json::Value>.
//! Uses batch processing to pre-compute column metadata once per result set.
//!
//! Supports type-aware decoding: when `col_types` from Python IR is provided,
//! uses those type hints instead of calling expensive type_info() per column.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use sqlx::{mysql::MySqlRow, Column, Row};
use std::collections::HashMap;

/// Column metadata cached for batch processing
struct ColumnMeta {
    name: String,
    /// Type from database schema (via type_info)
    db_type: String,
    /// Type hint from Python IR (when available)
    ir_type: Option<String>,
}

/// Convert multiple MySQL rows with cached column metadata.
/// Pre-computes column names and types from first row, then reuses for all rows.
#[allow(dead_code)]
pub fn convert_mysql_rows(rows: Vec<MySqlRow>) -> Vec<HashMap<String, serde_json::Value>> {
    convert_mysql_rows_typed(rows, None)
}

/// Convert multiple MySQL rows with optional type hints from Python IR.
/// When col_types is provided, uses those type hints for faster decoding
/// without calling type_info() per column.
pub fn convert_mysql_rows_typed(
    rows: Vec<MySqlRow>,
    col_types: Option<&HashMap<String, String>>,
) -> Vec<HashMap<String, serde_json::Value>> {
    if rows.is_empty() {
        return vec![];
    }

    // Pre-compute column info ONCE from first row
    let columns: Vec<ColumnMeta> = rows[0]
        .columns()
        .iter()
        .map(|c| {
            let name = Column::name(c).to_string();
            let ir_type = col_types.and_then(|ct| ct.get(&name).cloned());
            ColumnMeta {
                db_type: Column::type_info(c).to_string().to_uppercase(),
                ir_type,
                name,
            }
        })
        .collect();

    rows.into_iter()
        .map(|row| convert_row_with_meta(&row, &columns))
        .collect()
}

/// Convert a single row using pre-computed column metadata
fn convert_row_with_meta(
    row: &MySqlRow,
    columns: &[ColumnMeta],
) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::with_capacity(columns.len());
    for (i, col) in columns.iter().enumerate() {
        // Try IR type hint first (from Python), then fall back to DB type
        let value = if let Some(ir_type) = &col.ir_type {
            decode_mysql_cell_by_ir_type(row, i, ir_type)
                .unwrap_or_else(|| decode_mysql_cell(row, i, &col.db_type))
        } else {
            decode_mysql_cell(row, i, &col.db_type)
        };
        map.insert(col.name.clone(), value);
    }
    map
}

/// Decode a cell using Python IR type hint.
/// Returns None if the IR type is not recognized (fallback to DB type).
fn decode_mysql_cell_by_ir_type(
    row: &MySqlRow,
    idx: usize,
    ir_type: &str,
) -> Option<serde_json::Value> {
    match ir_type {
        "int" => Some(match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::Number(serde_json::Number::from(v)),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        }),
        "str" => Some(match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v),
            Ok(None) => serde_json::Value::Null,
            Err(_) => serde_json::Value::Null,
        }),
        "float" => Some(match row.try_get::<Option<f64>, _>(idx) {
            Ok(Some(v)) => serde_json::Number::from_f64(v)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        }),
        "bool" => Some(match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::Bool(v),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        }),
        "bytes" => Some(match row.try_get::<Option<Vec<u8>>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(BASE64_STANDARD.encode(v)),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        }),
        "datetime" => Some(match row.try_get::<Option<NaiveDateTime>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v.format("%Y-%m-%dT%H:%M:%S%.f").to_string()),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        }),
        "date" => Some(match row.try_get::<Option<NaiveDate>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v.format("%Y-%m-%d").to_string()),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        }),
        "time" => Some(match row.try_get::<Option<NaiveTime>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v.format("%H:%M:%S%.f").to_string()),
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        }),
        // uuid, decimal, timedelta - treat as string
        "uuid" | "decimal" | "timedelta" => Some(match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => serde_json::Value::String(v),
            Ok(None) => serde_json::Value::Null,
            Err(_) => serde_json::Value::Null,
        }),
        // JSON - MySQL has native JSON type
        "json" => Some(match row.try_get::<Option<serde_json::Value>, _>(idx) {
            Ok(Some(v)) => v,
            Ok(None) => serde_json::Value::Null,
            Err(_) => fallback_string_mysql(row, idx),
        }),
        _ => None, // Unknown IR type - fallback to DB type
    }
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

/// Columnar result: (column_names, rows_as_value_arrays)
pub type ColumnarResult = (Vec<String>, Vec<Vec<serde_json::Value>>);

/// Convert MySQL rows to columnar format.
/// Returns (column_names, rows) where each row is Vec<Value> in column order.
pub fn convert_mysql_rows_columnar(
    rows: Vec<MySqlRow>,
    col_types: Option<&HashMap<String, String>>,
) -> ColumnarResult {
    if rows.is_empty() {
        return (vec![], vec![]);
    }

    // Pre-compute column info ONCE from first row
    let columns: Vec<ColumnMeta> = rows[0]
        .columns()
        .iter()
        .map(|c| {
            let name = Column::name(c).to_string();
            let ir_type = col_types.and_then(|ct| ct.get(&name).cloned());
            ColumnMeta {
                db_type: Column::type_info(c).to_string().to_uppercase(),
                ir_type,
                name,
            }
        })
        .collect();

    // Extract column names
    let col_names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();

    // Convert rows to value arrays
    let row_values: Vec<Vec<serde_json::Value>> = rows
        .into_iter()
        .map(|row| convert_row_to_values(&row, &columns))
        .collect();

    (col_names, row_values)
}

/// Convert a single row to Vec<Value> using pre-computed column metadata
fn convert_row_to_values(row: &MySqlRow, columns: &[ColumnMeta]) -> Vec<serde_json::Value> {
    columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            if let Some(ir_type) = &col.ir_type {
                decode_mysql_cell_by_ir_type(row, i, ir_type)
                    .unwrap_or_else(|| decode_mysql_cell(row, i, &col.db_type))
            } else {
                decode_mysql_cell(row, i, &col.db_type)
            }
        })
        .collect()
}

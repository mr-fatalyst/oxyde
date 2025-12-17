//! Direct Row -> Py<PyAny> conversion without intermediate serde_json::Value.
//!
//! This module is only available when the `pyo3` feature is enabled.
//! It provides memory-efficient conversion by creating Python objects directly
//! from database rows, avoiding the ~25 MB overhead of Vec<Vec<JsonValue>>.
//!
//! ## Memory Impact
//!
//! Without this module (via JsonValue):
//! - SqliteRow (~25 MB) + Vec<Vec<JsonValue>> (~25 MB) + PyList (~20 MB) = ~70 MB peak
//!
//! With this module (direct conversion):
//! - SqliteRow (~25 MB) + PyList (~20 MB) = ~45 MB peak
//!
//! The difference is significant for large result sets (20K+ rows).

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyList, PyString};
use sqlx::{Column, Row};
use std::collections::HashMap;

use super::StreamingColumnMeta;

// ============================================================================
// SQLite conversion
// ============================================================================

use sqlx::sqlite::SqliteRow;

/// Convert SQLite rows directly to PyList[PyDict] without intermediate JsonValue.
///
/// This is the memory-efficient alternative to `convert_sqlite_rows_columnar`.
pub fn sqlite_rows_to_pylist<'py>(
    py: Python<'py>,
    rows: Vec<SqliteRow>,
    col_types: Option<&HashMap<String, String>>,
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);

    if rows.is_empty() {
        return Ok(result);
    }

    // Pre-compute column metadata ONCE from first row
    let columns = extract_sqlite_columns(&rows[0], col_types);

    // Convert each row directly to PyDict
    for row in rows {
        let dict = PyDict::new(py);
        for (i, col) in columns.iter().enumerate() {
            let value = decode_sqlite_cell_to_py(py, &row, i, col);
            dict.set_item(&col.name, value)?;
        }
        result.append(dict)?;
    }

    Ok(result)
}

/// Extract column metadata from SQLite row (used by both batch and streaming)
#[inline]
pub fn extract_sqlite_columns(
    row: &SqliteRow,
    col_types: Option<&HashMap<String, String>>,
) -> Vec<StreamingColumnMeta> {
    row.columns()
        .iter()
        .map(|c| {
            let name = Column::name(c).to_string();
            let ir_type = col_types.and_then(|ct| ct.get(&name).cloned());
            StreamingColumnMeta {
                db_type: Column::type_info(c).to_string().to_uppercase(),
                ir_type,
                name,
            }
        })
        .collect()
}

/// Decode a single SQLite cell to Python object using pre-computed metadata.
#[inline]
pub fn decode_sqlite_cell_to_py(
    py: Python<'_>,
    row: &SqliteRow,
    idx: usize,
    meta: &StreamingColumnMeta,
) -> Py<PyAny> {
    if let Some(ir_type) = &meta.ir_type {
        if let Some(val) = decode_sqlite_to_py_by_ir_type(py, row, idx, ir_type) {
            return val;
        }
    }
    decode_sqlite_to_py(py, row, idx, &meta.db_type)
}

/// Decode SQLite cell using Python IR type hint.
fn decode_sqlite_to_py_by_ir_type<'py>(
    py: Python<'py>,
    row: &SqliteRow,
    idx: usize,
    ir_type: &str,
) -> Option<Py<PyAny>> {
    match ir_type {
        // Explicit DB types (from Field(db_type="..."))
        // SQLite stores all integers as 64-bit, so all integer types use i64
        "INTEGER" | "INT" | "BIGINT" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT2" | "INT8" => {
            Some(match row.try_get::<Option<i64>, _>(idx) {
                Ok(Some(v)) => v.into_pyobject(py).unwrap().unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => fallback_string_py(py, row, idx),
            })
        }
        // Python type "int"
        "int" => Some(match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => v.into_pyobject(py).unwrap().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => fallback_string_py(py, row, idx),
        }),
        "str" | "TEXT" | "VARCHAR" | "CHAR" => Some(match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        }),
        "float" | "REAL" | "DOUBLE" | "DOUBLE PRECISION" | "FLOAT" => {
            Some(match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(v)) => PyFloat::new(py, v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => fallback_string_py(py, row, idx),
            })
        }
        "bool" | "BOOLEAN" => Some(match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => PyBool::new(py, v).to_owned().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => fallback_string_py(py, row, idx),
        }),
        "bytes" | "BLOB" => Some(match row.try_get::<Option<Vec<u8>>, _>(idx) {
            Ok(Some(v)) => PyBytes::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => fallback_string_py(py, row, idx),
        }),
        // datetime, date, time, timedelta, decimal, uuid - treat as string for SQLite
        "datetime" | "date" | "time" | "timedelta" | "decimal" | "uuid" => {
            Some(match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(v)) => PyString::new(py, &v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            })
        }
        _ => None, // Unknown IR type - fallback to DB type
    }
}

/// Decode SQLite cell to Py<PyAny> based on database type.
fn decode_sqlite_to_py(py: Python<'_>, row: &SqliteRow, idx: usize, type_name: &str) -> Py<PyAny> {
    match type_name {
        "BOOL" | "BOOLEAN" => match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => PyBool::new(py, v).to_owned().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => fallback_string_py(py, row, idx),
        },
        name if name.contains("INT") => match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => v.into_pyobject(py).unwrap().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => fallback_string_py(py, row, idx),
        },
        name if name.contains("REAL") || name.contains("FLOAT") || name.contains("DOUBLE") => {
            match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(v)) => PyFloat::new(py, v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => fallback_string_py(py, row, idx),
            }
        }
        name if name.contains("NUMERIC") || name.contains("DECIMAL") => {
            match row.try_get::<Option<String>, _>(idx) {
                Ok(Some(v)) => PyString::new(py, &v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => fallback_string_py(py, row, idx),
            }
        }
        name if name.contains("BLOB") => match row.try_get::<Option<Vec<u8>>, _>(idx) {
            Ok(Some(v)) => PyBytes::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => fallback_string_py(py, row, idx),
        },
        // SQLite dynamic typing: try INTEGER, REAL, TEXT, then NULL
        "NULL" => {
            if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(idx) {
                return v.into_pyobject(py).unwrap().unbind().into_any();
            }
            if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(idx) {
                return PyFloat::new(py, v).unbind().into_any();
            }
            if let Ok(Some(v)) = row.try_get::<Option<String>, _>(idx) {
                return PyString::new(py, &v).unbind().into_any();
            }
            py.None()
        }
        _ => match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => fallback_string_py(py, row, idx),
        },
    }
}

fn fallback_string_py(py: Python<'_>, row: &SqliteRow, idx: usize) -> Py<PyAny> {
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(v)) => PyString::new(py, &v).unbind().into_any(),
        Ok(None) => py.None(),
        Err(_) => py.None(),
    }
}

// ============================================================================
// PostgreSQL conversion
// ============================================================================

use sqlx::postgres::PgRow;

/// Convert PostgreSQL rows directly to PyList[PyDict].
pub fn pg_rows_to_pylist<'py>(
    py: Python<'py>,
    rows: Vec<PgRow>,
    col_types: Option<&HashMap<String, String>>,
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);

    if rows.is_empty() {
        return Ok(result);
    }

    // Pre-compute column metadata ONCE
    let columns = extract_pg_columns(&rows[0], col_types);

    for row in rows {
        let dict = PyDict::new(py);
        for (i, col) in columns.iter().enumerate() {
            let value = decode_pg_cell_to_py(py, &row, i, col);
            dict.set_item(&col.name, value)?;
        }
        result.append(dict)?;
    }

    Ok(result)
}

/// Extract column metadata from PostgreSQL row (used by both batch and streaming)
#[inline]
pub fn extract_pg_columns(
    row: &PgRow,
    col_types: Option<&HashMap<String, String>>,
) -> Vec<StreamingColumnMeta> {
    row.columns()
        .iter()
        .map(|c| {
            let name = Column::name(c).to_string();
            let ir_type = col_types.and_then(|ct| ct.get(&name).cloned());
            StreamingColumnMeta {
                db_type: Column::type_info(c).to_string().to_uppercase(),
                ir_type,
                name,
            }
        })
        .collect()
}

/// Decode a single PostgreSQL cell to Python object using pre-computed metadata.
#[inline]
pub fn decode_pg_cell_to_py(
    py: Python<'_>,
    row: &PgRow,
    idx: usize,
    meta: &StreamingColumnMeta,
) -> Py<PyAny> {
    decode_pg_to_py(py, row, idx, &meta.db_type, meta.ir_type.as_deref())
}

fn decode_pg_to_py(
    py: Python<'_>,
    row: &PgRow,
    idx: usize,
    db_type: &str,
    ir_type: Option<&str>,
) -> Py<PyAny> {
    // Try IR type first if available
    if let Some(ir_type) = ir_type {
        if let Some(val) = decode_pg_by_ir_type(py, row, idx, ir_type) {
            return val;
        }
    }

    // Fallback to DB type
    match db_type {
        "BOOL" | "BOOLEAN" => match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => PyBool::new(py, v).to_owned().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        name if name.contains("INT") => match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => v.into_pyobject(py).unwrap().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => match row.try_get::<Option<i32>, _>(idx) {
                Ok(Some(v)) => (v as i64).into_pyobject(py).unwrap().unbind().into_any(),
                _ => py.None(),
            },
        },
        name if name.contains("FLOAT") || name.contains("REAL") || name.contains("DOUBLE") => {
            match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(v)) => PyFloat::new(py, v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            }
        }
        "UUID" => match row.try_get::<Option<uuid::Uuid>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v.to_string()).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        "BYTEA" => match row.try_get::<Option<Vec<u8>>, _>(idx) {
            Ok(Some(v)) => PyBytes::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        "TIMESTAMPTZ" | "TIMESTAMP" => {
            match row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(idx) {
                Ok(Some(v)) => PyString::new(py, &v.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string())
                    .unbind()
                    .into_any(),
                Ok(None) => py.None(),
                Err(_) => match row.try_get::<Option<chrono::NaiveDateTime>, _>(idx) {
                    Ok(Some(v)) => {
                        PyString::new(py, &v.format("%Y-%m-%dT%H:%M:%S%.6f").to_string())
                            .unbind()
                            .into_any()
                    }
                    _ => py.None(),
                },
            }
        }
        "DATE" => match row.try_get::<Option<chrono::NaiveDate>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v.format("%Y-%m-%d").to_string())
                .unbind()
                .into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        "TIME" => match row.try_get::<Option<chrono::NaiveTime>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v.format("%H:%M:%S%.6f").to_string())
                .unbind()
                .into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        _ => match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
    }
}

fn decode_pg_by_ir_type(
    py: Python<'_>,
    row: &PgRow,
    idx: usize,
    ir_type: &str,
) -> Option<Py<PyAny>> {
    match ir_type {
        // Explicit DB types (from Field(db_type="..."))
        "BIGINT" | "INT8" | "BIGSERIAL" => Some(match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => v.into_pyobject(py).unwrap().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        }),
        "INTEGER" | "INT" | "INT4" | "SERIAL" | "SMALLINT" | "INT2" => {
            Some(match row.try_get::<Option<i32>, _>(idx) {
                Ok(Some(v)) => (v as i64).into_pyobject(py).unwrap().unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            })
        }
        // Python type "int" - try i32 first (common), fallback to i64
        "int" => Some(match row.try_get::<Option<i32>, _>(idx) {
            Ok(Some(v)) => (v as i64).into_pyobject(py).unwrap().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => match row.try_get::<Option<i64>, _>(idx) {
                Ok(Some(v)) => v.into_pyobject(py).unwrap().unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            },
        }),
        "str" | "TEXT" | "VARCHAR" | "CHAR" => Some(match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        }),
        "float" | "DOUBLE PRECISION" | "FLOAT8" | "REAL" | "FLOAT4" => {
            Some(match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(v)) => PyFloat::new(py, v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            })
        }
        "bool" | "BOOLEAN" | "BOOL" => Some(match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => PyBool::new(py, v).to_owned().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        }),
        "bytes" | "BYTEA" => Some(match row.try_get::<Option<Vec<u8>>, _>(idx) {
            Ok(Some(v)) => PyBytes::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        }),
        _ => None,
    }
}

// ============================================================================
// MySQL conversion
// ============================================================================

use sqlx::mysql::MySqlRow;

/// Convert MySQL rows directly to PyList[PyDict].
pub fn mysql_rows_to_pylist<'py>(
    py: Python<'py>,
    rows: Vec<MySqlRow>,
    col_types: Option<&HashMap<String, String>>,
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);

    if rows.is_empty() {
        return Ok(result);
    }

    // Pre-compute column metadata ONCE
    let columns = extract_mysql_columns(&rows[0], col_types);

    for row in rows {
        let dict = PyDict::new(py);
        for (i, col) in columns.iter().enumerate() {
            let value = decode_mysql_cell_to_py(py, &row, i, col);
            dict.set_item(&col.name, value)?;
        }
        result.append(dict)?;
    }

    Ok(result)
}

/// Extract column metadata from MySQL row (used by both batch and streaming)
#[inline]
pub fn extract_mysql_columns(
    row: &MySqlRow,
    col_types: Option<&HashMap<String, String>>,
) -> Vec<StreamingColumnMeta> {
    row.columns()
        .iter()
        .map(|c| {
            let name = Column::name(c).to_string();
            let ir_type = col_types.and_then(|ct| ct.get(&name).cloned());
            StreamingColumnMeta {
                db_type: Column::type_info(c).to_string().to_uppercase(),
                ir_type,
                name,
            }
        })
        .collect()
}

/// Decode a single MySQL cell to Python object using pre-computed metadata.
#[inline]
pub fn decode_mysql_cell_to_py(
    py: Python<'_>,
    row: &MySqlRow,
    idx: usize,
    meta: &StreamingColumnMeta,
) -> Py<PyAny> {
    decode_mysql_to_py(py, row, idx, &meta.db_type, meta.ir_type.as_deref())
}

fn decode_mysql_to_py(
    py: Python<'_>,
    row: &MySqlRow,
    idx: usize,
    db_type: &str,
    ir_type: Option<&str>,
) -> Py<PyAny> {
    // Try IR type first if available
    if let Some(ir_type) = ir_type {
        if let Some(val) = decode_mysql_by_ir_type(py, row, idx, ir_type) {
            return val;
        }
    }

    // Fallback to DB type
    match db_type {
        "BOOL" | "BOOLEAN" | "TINYINT(1)" => match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => PyBool::new(py, v).to_owned().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        name if name.contains("INT") => match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => v.into_pyobject(py).unwrap().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        name if name.contains("FLOAT") || name.contains("REAL") || name.contains("DOUBLE") => {
            match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(v)) => PyFloat::new(py, v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            }
        }
        "BLOB" | "LONGBLOB" | "MEDIUMBLOB" | "TINYBLOB" | "VARBINARY" | "BINARY" => {
            match row.try_get::<Option<Vec<u8>>, _>(idx) {
                Ok(Some(v)) => PyBytes::new(py, &v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            }
        }
        "DATETIME" | "TIMESTAMP" => match row.try_get::<Option<chrono::NaiveDateTime>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v.format("%Y-%m-%dT%H:%M:%S%.6f").to_string())
                .unbind()
                .into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        "DATE" => match row.try_get::<Option<chrono::NaiveDate>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v.format("%Y-%m-%d").to_string())
                .unbind()
                .into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        "TIME" => match row.try_get::<Option<chrono::NaiveTime>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v.format("%H:%M:%S%.6f").to_string())
                .unbind()
                .into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
        _ => match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        },
    }
}

fn decode_mysql_by_ir_type(
    py: Python<'_>,
    row: &MySqlRow,
    idx: usize,
    ir_type: &str,
) -> Option<Py<PyAny>> {
    match ir_type {
        // Explicit DB types (from Field(db_type="..."))
        "BIGINT" => Some(match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(v)) => v.into_pyobject(py).unwrap().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        }),
        // MySQL INT, INTEGER, MEDIUMINT are 32-bit; SMALLINT/TINYINT are smaller
        "INTEGER" | "INT" | "MEDIUMINT" | "SMALLINT" | "TINYINT" => {
            Some(match row.try_get::<Option<i32>, _>(idx) {
                Ok(Some(v)) => (v as i64).into_pyobject(py).unwrap().unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            })
        }
        // Python type "int" - try i32 first (common for PKs), fallback to i64
        "int" => Some(match row.try_get::<Option<i32>, _>(idx) {
            Ok(Some(v)) => (v as i64).into_pyobject(py).unwrap().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => match row.try_get::<Option<i64>, _>(idx) {
                Ok(Some(v)) => v.into_pyobject(py).unwrap().unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            },
        }),
        "str" | "TEXT" | "VARCHAR" | "CHAR" => Some(match row.try_get::<Option<String>, _>(idx) {
            Ok(Some(v)) => PyString::new(py, &v).unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        }),
        "float" | "DOUBLE" | "DOUBLE PRECISION" | "FLOAT" | "REAL" => {
            Some(match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(v)) => PyFloat::new(py, v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            })
        }
        "bool" | "BOOLEAN" | "BOOL" => Some(match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(v)) => PyBool::new(py, v).to_owned().unbind().into_any(),
            Ok(None) => py.None(),
            Err(_) => py.None(),
        }),
        "bytes" | "BLOB" | "LONGBLOB" | "MEDIUMBLOB" | "TINYBLOB" => {
            Some(match row.try_get::<Option<Vec<u8>>, _>(idx) {
                Ok(Some(v)) => PyBytes::new(py, &v).unbind().into_any(),
                Ok(None) => py.None(),
                Err(_) => py.None(),
            })
        }
        _ => None,
    }
}

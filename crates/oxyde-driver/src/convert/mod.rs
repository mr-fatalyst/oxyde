//! Type conversion utilities for database rows
//!
//! Each module provides batch conversion with pre-computed column metadata:
//! - `convert_*_rows(Vec<Row>)` - batch convert with cached column names/types
//! - `convert_*_rows_typed(Vec<Row>, Option<col_types>)` - type-aware batch convert
//! - `convert_*_rows_columnar(Vec<Row>, Option<col_types>)` - memory-efficient columnar format
//!
//! When `col_types` (from Python IR) is provided, uses type hints for decoding
//! instead of calling expensive `type_info()` per column.
//!
//! This avoids repeated `Column::name()` and `Column::type_info()` calls per cell.
//!
//! ## Columnar Format
//!
//! The columnar format `(Vec<String>, Vec<Vec<Value>>)` is more memory-efficient:
//! - Column names stored once, not repeated per row
//! - No HashMap overhead (~48 bytes per entry saved)
//! - Smaller msgpack serialization (no repeated keys)

pub mod mysql;
pub mod postgres;
pub mod sqlite;

#[cfg(feature = "pyo3")]
pub mod pyo3_convert;

pub use mysql::{convert_mysql_rows_columnar, convert_mysql_rows_typed};
pub use postgres::{convert_pg_rows, convert_pg_rows_columnar, convert_pg_rows_typed};
pub use sqlite::{convert_sqlite_rows, convert_sqlite_rows_columnar, convert_sqlite_rows_typed};

/// Column metadata for batched conversion
#[derive(Debug, Clone)]
pub struct StreamingColumnMeta {
    pub name: String,
    pub db_type: String,
    pub ir_type: Option<String>,
}

#[cfg(feature = "pyo3")]
pub use pyo3_convert::{
    decode_mysql_cell_to_py, decode_pg_cell_to_py, decode_sqlite_cell_to_py, extract_mysql_columns,
    extract_pg_columns, extract_sqlite_columns, mysql_rows_to_pylist, pg_rows_to_pylist,
    sqlite_rows_to_pylist,
};

/// Columnar result type alias for consistency across backends
pub type ColumnarResult = (Vec<String>, Vec<Vec<serde_json::Value>>);

//! Query Intermediate Representation (IR) types and MessagePack codec.
//!
//! This crate defines the data structures that represent queries in transit between
//! Python and Rust. Python builds IR dicts, serializes to MessagePack, and Rust
//! deserializes into these types for SQL generation.
//!
//! # Architecture
//!
//! ```text
//! Python Query → IR dict → msgpack bytes → QueryIR → SQL generation
//! ```
//!
//! # Core Types
//!
//! ## QueryIR
//! The main query representation. Contains:
//! - `proto`: Protocol version (must match IR_PROTO_VERSION)
//! - `op`: Operation type (Select, Insert, Update, Delete, Raw)
//! - `table`: Target table name
//! - `cols`: Columns to select
//! - `col_types`: Column type hints for type-aware decoding (SELECT only)
//! - `filter_tree`: WHERE clause as FilterNode tree
//! - `values`/`bulk_values`: INSERT/UPDATE data
//! - `order_by`, `limit`, `offset`: Pagination
//! - `joins`: JOIN specifications
//! - `aggregates`: COUNT, SUM, AVG, etc.
//! - `lock`: FOR UPDATE/FOR SHARE
//!
//! ## FilterNode
//! Represents WHERE clause as a tree:
//! - `Condition`: Leaf node (field, operator, value)
//! - `And`: Logical AND of children
//! - `Or`: Logical OR of children
//! - `Not`: Logical negation
//!
//! ## Aggregate
//! SQL aggregate functions:
//! - `op`: Count, Sum, Avg, Max, Min
//! - `field`: Column to aggregate
//! - `alias`: Result column name
//!
//! # Serialization
//!
//! ```rust,ignore
//! let ir = QueryIR::from_msgpack(bytes)?;
//! ir.validate()?; // Check structure, not data
//! // ... generate SQL
//! let result_bytes = serialize_results(rows)?;
//! ```
//!
//! # Validation
//!
//! `QueryIR::validate()` checks IR structure only:
//! - Protocol version compatibility
//! - Required fields for operation type
//! - No data validation (handled by Pydantic in Python)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during IR serialization, deserialization, or validation.
#[derive(Debug, Error)]
pub enum CodecError {
    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },
}

pub type Result<T> = std::result::Result<T, CodecError>;

/// IR protocol version
pub const IR_PROTO_VERSION: u32 = 1;

/// Canonical column-type contract between Python and the Rust core.
///
/// This is the single semantic taxonomy used for value binding (oxyde-query),
/// row decoding (oxyde-driver) and canonical DDL rendering (oxyde-migrate).
/// Python computes it once per column from the field annotation and `db_type`
/// (see `core/column_types.py`) and sends it as a tagged dict, e.g.
/// `{"kind": "decimal", "precision": 10, "scale": 2}`.
///
/// The user-supplied verbatim DDL override (`db_type`) is NOT part of this
/// enum — it travels separately in `FieldDef.db_type` and is only ever
/// rendered, never classified. Unrecognized `db_type` strings map to
/// [`ColumnTypeSpec::Unknown`]: values bind via native msgpack-type
/// conversion, exactly like columns without a type hint today.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ColumnTypeSpec {
    /// Python `int`. Always bound as i64; DDL width is a dialect decision.
    BigInteger,
    /// Python `float` (f64).
    Double,
    Boolean,
    /// Unbounded text (Python `str` without max_length → VARCHAR is a DDL
    /// concern; binding-wise both are strings).
    Text,
    /// Bounded string (VARCHAR(n) / CHAR(n) family).
    String {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<u32>,
    },
    /// Python `bytes`.
    Blob,
    /// Naive datetime (no timezone).
    DateTime,
    /// Timezone-aware datetime, normalized to UTC. Selected explicitly via
    /// `db_type="TIMESTAMPTZ"`.
    DateTimeUtc,
    Date,
    Time,
    /// Python `timedelta`, stored as BIGINT microseconds.
    Timedelta,
    Uuid,
    Decimal {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        precision: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<u32>,
    },
    Json,
    /// JSONB on Postgres; identical to Json elsewhere.
    JsonBinary,
    Array {
        item: Box<ColumnTypeSpec>,
    },
    /// No binding knowledge: convert values natively by their msgpack type.
    /// Produced for unrecognized `db_type` strings.
    Unknown,
}

/// Query operation type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Select,
    Insert,
    Update,
    Delete,
    Raw,
}

/// Filter condition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub field: String,
    pub operator: String,
    pub value: rmpv::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escape: Option<String>,
}

/// Filter node for complex logical expressions (Q-expressions)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FilterNode {
    #[serde(rename = "condition")]
    Condition(Filter),
    #[serde(rename = "and")]
    And { conditions: Vec<FilterNode> },
    #[serde(rename = "or")]
    Or { conditions: Vec<FilterNode> },
    #[serde(rename = "not")]
    Not { condition: Box<FilterNode> },
}

/// Aggregate operation type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AggregateOp {
    Count,
    Sum,
    Avg,
    Max,
    Min,
}

/// Aggregate specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Aggregate {
    pub op: AggregateOp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distinct: Option<bool>,
}

/// Lock type for pessimistic locking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LockType {
    Update,
    Share,
}

/// ON CONFLICT action
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConflictAction {
    Nothing,
    Update,
}

/// ON CONFLICT specification for UPSERT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnConflict {
    /// Conflict target columns (e.g., ["email"])
    pub columns: Vec<String>,
    /// Action to take on conflict
    pub action: ConflictAction,
    /// Update values (only for action=Update)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_values: Option<std::collections::HashMap<String, rmpv::Value>>,
}

/// Join column projection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinColumn {
    pub field: String,
    pub column: String,
}

/// Join specification for SELECT queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinSpec {
    pub path: String,
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub table: String,
    pub source_column: String,
    pub target_column: String,
    pub result_prefix: String,
    pub columns: Vec<JoinColumn>,
}

/// Query IR structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryIR {
    pub proto: u32,
    pub op: Operation,
    pub table: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cols: Option<Vec<String>>,

    /// Column type specs from Python for typed binding and decoding.
    /// Maps column name to `ColumnTypeSpec` (tagged dict on the wire).
    /// When present, Rust binds values and decodes rows without expensive
    /// type_info() calls or string classification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_types: Option<HashMap<String, ColumnTypeSpec>>,

    // Filters using FilterNode tree (supports AND/OR/NOT logic)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_tree: Option<FilterNode>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_by: Option<Vec<(String, String)>>,

    // Single row values (for simple INSERT/UPDATE)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<HashMap<String, rmpv::Value>>,

    // Bulk values (for bulk INSERT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bulk_values: Option<Vec<HashMap<String, rmpv::Value>>>,

    // Bulk update payload
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bulk_update: Option<BulkUpdate>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distinct: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_mappings: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joins: Option<Vec<JoinSpec>>,

    // Aggregates (for COUNT, SUM, AVG, MAX, MIN)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregates: Option<Vec<Aggregate>>,

    // RETURNING clause for INSERT/UPDATE/DELETE
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returning: Option<bool>,

    // GROUP BY clause
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_by: Option<Vec<String>>,

    // HAVING clause (uses FilterNode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub having: Option<FilterNode>,

    // EXISTS flag (wraps query in SELECT EXISTS(...))
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,

    // COUNT flag (returns SELECT COUNT(*) instead of rows)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<bool>,

    // ON CONFLICT clause for UPSERT (INSERT only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_conflict: Option<OnConflict>,

    // Pessimistic locking (FOR UPDATE / FOR SHARE)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock: Option<LockType>,

    // UNION query (another QueryIR to union with)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub union_query: Option<Box<QueryIR>>,

    // UNION ALL flag (if false, use UNION DISTINCT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub union_all: Option<bool>,

    // Raw SQL query (for operation = Raw)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sql: Option<String>,

    // Raw SQL parameters (for operation = Raw)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<rmpv::Value>>,

    // Primary key column name for INSERT RETURNING
    // Used to generate proper RETURNING clause instead of hardcoded "id"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pk_column: Option<String>,
}

/// Single row in a bulk update: filters identify the row, values are the new data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkUpdateRow {
    pub filters: HashMap<String, rmpv::Value>,
    pub values: HashMap<String, rmpv::Value>,
}

/// Bulk update payload: multiple rows, each with its own filters and values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkUpdate {
    pub rows: Vec<BulkUpdateRow>,
}

impl Default for QueryIR {
    fn default() -> Self {
        Self {
            proto: IR_PROTO_VERSION,
            op: Operation::Select,
            table: String::new(),
            cols: None,
            column_types: None,
            filter_tree: None,
            limit: None,
            offset: None,
            order_by: None,
            values: None,
            bulk_values: None,
            bulk_update: None,
            model: None,
            distinct: None,
            column_mappings: None,
            joins: None,
            aggregates: None,
            returning: None,
            group_by: None,
            having: None,
            exists: None,
            count: None,
            on_conflict: None,
            lock: None,
            union_query: None,
            union_all: None,
            sql: None,
            params: None,
            pk_column: None,
        }
    }
}

impl QueryIR {
    /// Parse IR from MessagePack bytes
    pub fn from_msgpack(bytes: &[u8]) -> Result<Self> {
        rmp_serde::from_slice(bytes).map_err(|e| {
            CodecError::DeserializationError(format!("Failed to parse MessagePack: {}", e))
        })
    }

    /// Validate IR structure (NOT data values - those are validated in Python via Pydantic)
    ///
    /// This only checks:
    /// - Protocol version compatibility
    /// - Required fields for each operation type
    /// - IR structure integrity
    pub fn validate(&self) -> Result<()> {
        // Protocol version check
        if self.proto != IR_PROTO_VERSION {
            return Err(CodecError::ValidationError(format!(
                "Unsupported protocol version: expected {}, got {}",
                IR_PROTO_VERSION, self.proto
            )));
        }

        // Validate operation-specific requirements (structure only)
        match self.op {
            Operation::Select => {
                // SELECT can have cols, aggregates, count, or exists
                if self.cols.is_none()
                    && self.aggregates.is_none()
                    && !self.count.unwrap_or(false)
                    && !self.exists.unwrap_or(false)
                {
                    return Err(CodecError::ValidationError(
                        "SELECT query must specify columns, aggregate, count, or exists"
                            .to_string(),
                    ));
                }
            }
            Operation::Insert => {
                // INSERT must have either values or bulk_values
                if self.values.is_none() && self.bulk_values.is_none() {
                    return Err(CodecError::ValidationError(
                        "INSERT query must specify values or bulk_values".to_string(),
                    ));
                }
                if self.values.is_some() && self.bulk_values.is_some() {
                    return Err(CodecError::ValidationError(
                        "INSERT query cannot specify both values and bulk_values".to_string(),
                    ));
                }
            }
            Operation::Update => {
                if self.values.is_none() && self.bulk_update.is_none() {
                    return Err(CodecError::ValidationError(
                        "UPDATE query must specify values or bulk_update".to_string(),
                    ));
                }
                if let Some(bulk) = &self.bulk_update {
                    if bulk.rows.is_empty() {
                        return Err(CodecError::ValidationError(
                            "bulk_update requires at least one row".to_string(),
                        ));
                    }
                    for row in &bulk.rows {
                        if row.filters.is_empty() {
                            return Err(CodecError::ValidationError(
                                "bulk_update rows require at least one filter".to_string(),
                            ));
                        }
                        if row.values.is_empty() {
                            return Err(CodecError::ValidationError(
                                "bulk_update rows require at least one value".to_string(),
                            ));
                        }
                    }
                }
                if self.values.is_some() && self.bulk_update.is_some() {
                    return Err(CodecError::ValidationError(
                        "UPDATE query cannot specify both values and bulk_update".to_string(),
                    ));
                }
            }
            Operation::Delete => {
                // No specific requirements for DELETE
            }
            Operation::Raw => {
                // Raw SQL must have sql field
                if self.sql.is_none() {
                    return Err(CodecError::ValidationError(
                        "RAW query must specify sql".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_ir_validation() {
        let ir = QueryIR {
            table: "users".into(),
            cols: Some(vec!["id".into(), "name".into()]),
            ..Default::default()
        };
        assert!(ir.validate().is_ok());
    }

    #[test]
    fn test_query_ir_select_without_columns_is_error() {
        let ir = QueryIR {
            table: "users".into(),
            ..Default::default()
        };
        let err = ir.validate().unwrap_err();
        assert!(matches!(err, CodecError::ValidationError(msg) if msg.contains("columns")));
    }

    #[test]
    fn test_query_ir_insert_requires_values() {
        let ir = QueryIR {
            op: Operation::Insert,
            table: "users".into(),
            ..Default::default()
        };
        let err = ir.validate().unwrap_err();
        assert!(matches!(err, CodecError::ValidationError(msg) if msg.contains("INSERT")));
    }

    // ── ColumnTypeSpec serialization ───────────────────────────────────

    /// Deserialize a JSON literal shaped exactly like the dict Python sends.
    fn spec_from_json(json: &str) -> ColumnTypeSpec {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn test_spec_scalar_kinds_from_python_dicts() {
        let cases = [
            (r#"{"kind": "big_integer"}"#, ColumnTypeSpec::BigInteger),
            (r#"{"kind": "double"}"#, ColumnTypeSpec::Double),
            (r#"{"kind": "boolean"}"#, ColumnTypeSpec::Boolean),
            (r#"{"kind": "text"}"#, ColumnTypeSpec::Text),
            (r#"{"kind": "blob"}"#, ColumnTypeSpec::Blob),
            (r#"{"kind": "date_time"}"#, ColumnTypeSpec::DateTime),
            (r#"{"kind": "date_time_utc"}"#, ColumnTypeSpec::DateTimeUtc),
            (r#"{"kind": "date"}"#, ColumnTypeSpec::Date),
            (r#"{"kind": "time"}"#, ColumnTypeSpec::Time),
            (r#"{"kind": "timedelta"}"#, ColumnTypeSpec::Timedelta),
            (r#"{"kind": "uuid"}"#, ColumnTypeSpec::Uuid),
            (r#"{"kind": "json"}"#, ColumnTypeSpec::Json),
            (r#"{"kind": "json_binary"}"#, ColumnTypeSpec::JsonBinary),
            (r#"{"kind": "unknown"}"#, ColumnTypeSpec::Unknown),
        ];
        for (json, expected) in cases {
            assert_eq!(spec_from_json(json), expected, "input: {json}");
        }
    }

    #[test]
    fn test_spec_string_with_and_without_length() {
        assert_eq!(
            spec_from_json(r#"{"kind": "string", "length": 100}"#),
            ColumnTypeSpec::String { length: Some(100) }
        );
        assert_eq!(
            spec_from_json(r#"{"kind": "string"}"#),
            ColumnTypeSpec::String { length: None }
        );
    }

    #[test]
    fn test_spec_decimal_optional_precision() {
        assert_eq!(
            spec_from_json(r#"{"kind": "decimal", "precision": 10, "scale": 2}"#),
            ColumnTypeSpec::Decimal {
                precision: Some(10),
                scale: Some(2),
            }
        );
        assert_eq!(
            spec_from_json(r#"{"kind": "decimal"}"#),
            ColumnTypeSpec::Decimal {
                precision: None,
                scale: None,
            }
        );
    }

    #[test]
    fn test_spec_nested_array() {
        assert_eq!(
            spec_from_json(r#"{"kind": "array", "item": {"kind": "uuid"}}"#),
            ColumnTypeSpec::Array {
                item: Box::new(ColumnTypeSpec::Uuid),
            }
        );
        // Arrays nest (Postgres allows multi-dimensional arrays)
        assert_eq!(
            spec_from_json(
                r#"{"kind": "array", "item": {"kind": "array", "item": {"kind": "big_integer"}}}"#
            ),
            ColumnTypeSpec::Array {
                item: Box::new(ColumnTypeSpec::Array {
                    item: Box::new(ColumnTypeSpec::BigInteger),
                }),
            }
        );
    }

    #[test]
    fn test_spec_unknown_kind_is_error() {
        let result: std::result::Result<ColumnTypeSpec, _> =
            serde_json::from_str(r#"{"kind": "flux_capacitor"}"#);
        assert!(result.is_err(), "unrecognized kind must fail loudly");
    }

    #[test]
    fn test_spec_msgpack_roundtrip() {
        let specs = vec![
            ColumnTypeSpec::BigInteger,
            ColumnTypeSpec::String { length: Some(36) },
            ColumnTypeSpec::Decimal {
                precision: Some(10),
                scale: Some(2),
            },
            ColumnTypeSpec::Array {
                item: Box::new(ColumnTypeSpec::Uuid),
            },
            ColumnTypeSpec::Unknown,
        ];
        for spec in specs {
            let bytes = rmp_serde::to_vec_named(&spec).unwrap();
            let back: ColumnTypeSpec = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(back, spec);
        }
    }

    #[test]
    fn test_spec_in_column_types_map_via_msgpack() {
        // Simulate the future QueryIR.column_types payload: a msgpack map of
        // column name → tagged spec dict, exactly as Python will send it.
        let mut map = HashMap::new();
        map.insert(
            "price".to_string(),
            ColumnTypeSpec::Decimal {
                precision: Some(10),
                scale: Some(2),
            },
        );
        map.insert("id".to_string(), ColumnTypeSpec::BigInteger);

        let bytes = rmp_serde::to_vec_named(&map).unwrap();
        let back: HashMap<String, ColumnTypeSpec> = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(back, map);
    }
}

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
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
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
    pub update_values: Option<std::collections::HashMap<String, serde_json::Value>>,
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

    /// Column type hints from Python for type-aware decoding.
    /// Maps column name to IR type string: "int", "str", "float", "bool",
    /// "bytes", "datetime", "date", "time", "timedelta", "decimal", "uuid".
    /// When present, Rust can decode values without expensive type_info() calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col_types: Option<HashMap<String, String>>,

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
    pub values: Option<HashMap<String, serde_json::Value>>,

    // Bulk values (for bulk INSERT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bulk_values: Option<Vec<HashMap<String, serde_json::Value>>>,

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
    pub params: Option<Vec<serde_json::Value>>,

    // Primary key column name for INSERT RETURNING
    // Used to generate proper RETURNING clause instead of hardcoded "id"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pk_column: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkUpdateRow {
    pub filters: HashMap<String, serde_json::Value>,
    pub values: HashMap<String, serde_json::Value>,
}

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
            col_types: None,
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

/// Columnar result format: (column_names, rows_as_arrays)
/// More memory-efficient than Vec<HashMap>:
/// - Column names stored once, not per row
/// - No HashMap overhead (~48 bytes per entry)
/// - ~30% smaller msgpack serialization
pub type ColumnarResult = (Vec<String>, Vec<Vec<serde_json::Value>>);

/// Serialize columnar results to MessagePack
/// Format: [columns, rows] where rows is array of arrays
pub fn serialize_columnar_results(result: ColumnarResult) -> Result<Vec<u8>> {
    rmp_serde::to_vec(&result)
        .map_err(|e| CodecError::SerializationError(format!("Failed to serialize: {}", e)))
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
}

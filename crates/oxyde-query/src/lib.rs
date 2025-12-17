//! SQL generation from QueryIR using sea_query.
//!
//! This crate converts QueryIR structures (from oxyde-codec) into dialect-specific
//! SQL strings with bound parameters. It uses the sea_query library as the SQL AST
//! builder, providing type-safe SQL generation across PostgreSQL, SQLite, and MySQL.
//!
//! # Main Function
//!
//! ```rust,ignore
//! use oxyde_query::{build_sql, Dialect};
//!
//! let (sql, values) = build_sql(&ir, Dialect::Postgres)?;
//! // sql: "SELECT \"id\", \"name\" FROM \"users\" WHERE \"age\" >= $1"
//! // values: [Value::Int(18)]
//! ```
//!
//! # Dialect Support
//!
//! - **PostgreSQL**: `$1, $2, $3` placeholders, RETURNING clause, FOR UPDATE/SHARE
//! - **SQLite**: `?1, ?2, ?3` placeholders, RETURNING (3.35+), no row-level locking
//! - **MySQL**: `?, ?, ?` placeholders, no RETURNING, FOR UPDATE/SHARE
//!
//! # Architecture
//!
//! The crate is organized into modules:
//! - `builder` - Main SQL builders for SELECT, INSERT, UPDATE, DELETE
//! - `filter` - WHERE clause from FilterNode tree (AND/OR/NOT, operators)
//! - `aggregate` - COUNT, SUM, AVG, MAX, MIN handling
//! - `utils` - JSON value to sea_query::Value conversion
//! - `error` - QueryError types
//!
//! # Supported Features
//!
//! - SELECT with JOIN, GROUP BY, HAVING, ORDER BY, LIMIT, OFFSET
//! - INSERT single row and bulk insert
//! - UPDATE with SET, WHERE, and bulk CASE WHEN
//! - DELETE with WHERE
//! - Raw SQL pass-through
//! - Aggregate functions
//! - Row-level locking (FOR UPDATE/FOR SHARE)
//! - UNION / UNION ALL

use oxyde_codec::{Operation, QueryIR};
use sea_query::Value;

// Module declarations
pub mod aggregate;
pub mod builder;
pub mod error;
pub mod filter;
pub mod utils;

// Re-exports
pub use error::{QueryError, Result};

/// Database dialect
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Postgres,
    Sqlite,
    Mysql,
}

impl Dialect {
    pub fn from_url(url: &str) -> Self {
        if url.starts_with("postgres") {
            Dialect::Postgres
        } else if url.starts_with("sqlite") {
            Dialect::Sqlite
        } else if url.starts_with("mysql") {
            Dialect::Mysql
        } else {
            Dialect::Postgres // default
        }
    }
}

/// Build SQL from QueryIR
///
/// This is the main entry point for SQL generation. It dispatches to the
/// appropriate builder based on the operation type.
pub fn build_sql(ir: &QueryIR, dialect: Dialect) -> Result<(String, Vec<Value>)> {
    match ir.op {
        Operation::Select => builder::build_select(ir, dialect),
        Operation::Insert => builder::build_insert(ir, dialect),
        Operation::Update => builder::build_update(ir, dialect),
        Operation::Delete => builder::build_delete(ir, dialect),
        Operation::Raw => {
            // Raw SQL - just return the SQL as-is with parameters
            let sql = ir
                .sql
                .as_ref()
                .ok_or_else(|| QueryError::InvalidQuery("Raw query missing sql field".into()))?
                .clone();

            // Convert params to sea_query Values
            let values = if let Some(params) = &ir.params {
                params.iter().map(utils::json_to_value).collect()
            } else {
                Vec::new()
            };

            Ok((sql, values))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxyde_codec::{
        ConflictAction, Filter, FilterNode, JoinColumn, JoinSpec, OnConflict, Operation, QueryIR,
    };
    use serde_json::json;
    use std::collections::HashMap;

    /// Helper to create a simple condition filter node
    fn filter_cond(field: &str, operator: &str, value: serde_json::Value) -> FilterNode {
        FilterNode::Condition(Filter {
            field: field.into(),
            operator: operator.into(),
            value,
            column: None,
        })
    }

    /// Helper to create a filter with column alias
    fn filter_with_column(
        field: &str,
        column: &str,
        operator: &str,
        value: serde_json::Value,
    ) -> FilterNode {
        FilterNode::Condition(Filter {
            field: field.into(),
            operator: operator.into(),
            value,
            column: Some(column.into()),
        })
    }

    #[test]
    fn test_select_query() {
        let ir = QueryIR {
            table: "users".into(),
            cols: Some(vec!["id".into(), "name".into()]),
            limit: Some(10),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.contains("SELECT"));
        assert!(sql.contains("users"));
        assert!(sql.contains("LIMIT"));
    }

    #[test]
    fn test_select_with_params() {
        let ir = QueryIR {
            table: "users".into(),
            cols: Some(vec!["id".into()]),
            filter_tree: Some(filter_cond("id", "=", json!(42))),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.contains("$1"));
        assert_eq!(params.len(), 1);
        match &params[0] {
            Value::BigInt(Some(v)) => assert_eq!(*v, 42),
            other => panic!("unexpected param value: {:?}", other),
        }
    }

    #[test]
    fn test_mysql_placeholders() {
        let ir = QueryIR {
            table: "widgets".into(),
            cols: Some(vec!["id".into()]),
            filter_tree: Some(filter_cond("slug", "=", json!("foo"))),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Mysql).unwrap();
        assert!(sql.contains("?"));
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn test_sqlite_placeholders() {
        let ir = QueryIR {
            op: Operation::Insert,
            table: "widgets".into(),
            values: Some(HashMap::from([("name".into(), json!("foo"))])),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Sqlite).unwrap();
        assert!(sql.contains("?"));
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn test_select_ilike_lookup() {
        let ir = QueryIR {
            table: "articles".into(),
            cols: Some(vec!["id".into()]),
            filter_tree: Some(filter_cond("title", "ILIKE", json!("%rust%"))),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.to_uppercase().contains("LIKE"));
        assert_eq!(params.len(), 1);
        match &params[0] {
            Value::String(Some(val)) => assert_eq!(val.as_ref(), "%rust%"),
            other => panic!("unexpected parameter value {:?}", other),
        }
    }

    #[test]
    fn test_select_between_lookup() {
        let ir = QueryIR {
            table: "numbers".into(),
            cols: Some(vec!["value".into()]),
            filter_tree: Some(filter_cond("value", "BETWEEN", json!([1, 5]))),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.to_uppercase().contains("BETWEEN"));
        assert!(sql.contains("$1") && sql.contains("$2"));
    }

    #[test]
    fn test_select_is_null_lookup() {
        let ir = QueryIR {
            table: "entries".into(),
            cols: Some(vec!["id".into()]),
            filter_tree: Some(filter_cond("deleted_at", "IS NULL", json!(null))),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.to_uppercase().contains("IS NULL"));
    }

    #[test]
    fn test_column_mappings_emit_aliases() {
        let ir = QueryIR {
            table: "posts".into(),
            cols: Some(vec!["title".into()]),
            column_mappings: Some(HashMap::from([("title".into(), "title_text".into())])),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(
            sql.contains("\"title_text\" AS \"title\""),
            "unexpected SQL: {}",
            sql
        );
    }

    #[test]
    fn test_join_spec_adds_left_join() {
        let join = JoinSpec {
            path: "author".into(),
            alias: "author".into(),
            parent: None,
            table: "authors".into(),
            source_column: "author_id".into(),
            target_column: "id".into(),
            result_prefix: "author".into(),
            columns: vec![
                JoinColumn {
                    field: "id".into(),
                    column: "id".into(),
                },
                JoinColumn {
                    field: "name".into(),
                    column: "name".into(),
                },
            ],
        };
        let ir = QueryIR {
            table: "posts".into(),
            cols: Some(vec!["title".into()]),
            joins: Some(vec![join]),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
        let upper = sql.to_uppercase();
        assert!(upper.contains("LEFT JOIN"), "{}", sql);
        assert!(sql.contains("\"posts\".\"author_id\""), "{}", sql);
        assert!(sql.contains("\"author\".\"id\""), "{}", sql);
        assert!(sql.contains("author__id"), "{}", sql);
        assert!(sql.contains("author__name"), "{}", sql);
    }

    #[test]
    fn test_on_conflict_update_requires_values() {
        let ir = QueryIR {
            op: Operation::Insert,
            table: "users".into(),
            values: Some(HashMap::from([("email".into(), json!("test@example.com"))])),
            on_conflict: Some(OnConflict {
                columns: vec!["email".into()],
                action: ConflictAction::Update,
                update_values: None, // No values - should fail
            }),
            ..Default::default()
        };
        let result = build_sql(&ir, Dialect::Postgres);
        assert!(
            result.is_err(),
            "on_conflict Update without values should fail"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("update_values"),
            "error should mention update_values: {}",
            err
        );
    }

    #[test]
    fn test_ilike_uses_column_alias() {
        let ir = QueryIR {
            table: "products".into(),
            cols: Some(vec!["id".into(), "name".into()]),
            filter_tree: Some(filter_with_column(
                "name",
                "product_name",
                "ILIKE",
                json!("%test%"),
            )),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(
            sql.contains("product_name") || sql.contains("\"product_name\""),
            "ILIKE should use column alias 'product_name', got: {}",
            sql
        );
    }

    #[test]
    fn test_mysql_on_duplicate_key_update() {
        let ir = QueryIR {
            op: Operation::Insert,
            table: "items".into(),
            values: Some(HashMap::from([
                ("id".into(), json!(1)),
                ("name".into(), json!("test")),
                ("count".into(), json!(10)),
            ])),
            on_conflict: Some(OnConflict {
                columns: vec!["id".into()],
                action: ConflictAction::Update,
                update_values: Some(HashMap::from([
                    ("name".into(), json!("updated")),
                    ("count".into(), json!(20)),
                ])),
            }),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Mysql).unwrap();
        assert!(
            sql.contains("ON DUPLICATE KEY UPDATE"),
            "MySQL should use ON DUPLICATE KEY UPDATE, got: {}",
            sql
        );
        assert!(params.len() >= 3, "Should have insert params");
    }

    #[test]
    fn test_mysql_on_duplicate_key_nothing() {
        let ir = QueryIR {
            op: Operation::Insert,
            table: "items".into(),
            values: Some(HashMap::from([
                ("id".into(), json!(1)),
                ("name".into(), json!("test")),
            ])),
            on_conflict: Some(OnConflict {
                columns: vec!["id".into()],
                action: ConflictAction::Nothing,
                update_values: None,
            }),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Mysql).unwrap();
        assert!(
            sql.contains("ON DUPLICATE KEY UPDATE"),
            "MySQL DoNothing should use ON DUPLICATE KEY UPDATE (no-op), got: {}",
            sql
        );
    }
}

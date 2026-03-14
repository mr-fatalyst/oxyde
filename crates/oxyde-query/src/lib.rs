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
                params.iter().map(utils::rmpv_to_value).collect()
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
    use std::collections::HashMap;

    fn rmpv_int(v: i64) -> rmpv::Value {
        rmpv::Value::Integer(v.into())
    }

    fn rmpv_str(v: &str) -> rmpv::Value {
        rmpv::Value::String(v.into())
    }

    fn rmpv_null() -> rmpv::Value {
        rmpv::Value::Nil
    }

    fn rmpv_arr(vals: Vec<rmpv::Value>) -> rmpv::Value {
        rmpv::Value::Array(vals)
    }

    /// Helper to create a simple condition filter node
    fn filter_cond(field: &str, operator: &str, value: rmpv::Value) -> FilterNode {
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
        value: rmpv::Value,
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
            filter_tree: Some(filter_cond("id", "=", rmpv_int(42))),
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
            filter_tree: Some(filter_cond("slug", "=", rmpv_str("foo"))),
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
            values: Some(HashMap::from([("name".into(), rmpv_str("foo"))])),
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
            filter_tree: Some(filter_cond("title", "ILIKE", rmpv_str("%rust%"))),
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
            filter_tree: Some(filter_cond(
                "value",
                "BETWEEN",
                rmpv_arr(vec![rmpv_int(1), rmpv_int(5)]),
            )),
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
            filter_tree: Some(filter_cond("deleted_at", "IS NULL", rmpv_null())),
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
            values: Some(HashMap::from([(
                "email".into(),
                rmpv_str("test@example.com"),
            )])),
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
                rmpv_str("%test%"),
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
                ("id".into(), rmpv_int(1)),
                ("name".into(), rmpv_str("test")),
                ("count".into(), rmpv_int(10)),
            ])),
            on_conflict: Some(OnConflict {
                columns: vec!["id".into()],
                action: ConflictAction::Update,
                update_values: Some(HashMap::from([
                    ("name".into(), rmpv_str("updated")),
                    ("count".into(), rmpv_int(20)),
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
                ("id".into(), rmpv_int(1)),
                ("name".into(), rmpv_str("test")),
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

    #[test]
    fn test_union_order_by_after_union() {
        let union_ir = QueryIR {
            table: "posts".into(),
            cols: Some(vec!["id".into(), "title".into()]),
            filter_tree: Some(filter_cond("published", "=", rmpv_int(1))),
            ..Default::default()
        };
        let ir = QueryIR {
            table: "posts".into(),
            cols: Some(vec!["id".into(), "title".into()]),
            filter_tree: Some(filter_cond("author_id", "=", rmpv_int(1))),
            union_query: Some(Box::new(union_ir)),
            order_by: Some(vec![("id".into(), "ASC".into())]),
            ..Default::default()
        };

        for dialect in [Dialect::Sqlite, Dialect::Postgres, Dialect::Mysql] {
            let (sql, _) = build_sql(&ir, dialect).unwrap();
            let union_pos = sql.find("UNION").expect("should contain UNION");
            let order_pos = sql.find("ORDER BY").expect("should contain ORDER BY");
            assert!(
                order_pos > union_pos,
                "{dialect:?}: ORDER BY should come after UNION.\nSQL: {sql}"
            );
        }
    }

    #[test]
    fn test_union_all_generates_correct_sql() {
        let union_ir = QueryIR {
            table: "posts".into(),
            cols: Some(vec!["id".into()]),
            filter_tree: Some(filter_cond("id", "=", rmpv_int(3))),
            ..Default::default()
        };
        let ir = QueryIR {
            table: "posts".into(),
            cols: Some(vec!["id".into()]),
            filter_tree: Some(filter_cond("id", "=", rmpv_int(1))),
            union_query: Some(Box::new(union_ir)),
            union_all: Some(true),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Sqlite).unwrap();
        assert!(sql.contains("UNION ALL"), "should contain UNION ALL: {sql}");
        assert_eq!(params.len(), 2, "should have 2 params: {params:?}");
    }

    #[test]
    fn test_union_with_postgres_placeholders() {
        let union_ir = QueryIR {
            table: "posts".into(),
            cols: Some(vec!["id".into()]),
            filter_tree: Some(filter_cond("status", "=", rmpv_str("draft"))),
            ..Default::default()
        };
        let ir = QueryIR {
            table: "posts".into(),
            cols: Some(vec!["id".into()]),
            filter_tree: Some(filter_cond("status", "=", rmpv_str("active"))),
            union_query: Some(Box::new(union_ir)),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.contains("$1"), "should have $1: {sql}");
        assert!(sql.contains("$2"), "should have $2: {sql}");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_count_distinct() {
        let ir = QueryIR {
            table: "users".into(),
            aggregates: Some(vec![oxyde_codec::Aggregate {
                op: oxyde_codec::AggregateOp::Count,
                field: Some("email".into()),
                alias: Some("unique_emails".into()),
                distinct: Some(true),
            }]),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(
            sql.contains("COUNT(DISTINCT"),
            "should have COUNT(DISTINCT: {sql}"
        );
        assert!(sql.contains("unique_emails"), "should have alias: {sql}");
    }

    #[test]
    fn test_sum_distinct() {
        let ir = QueryIR {
            table: "orders".into(),
            aggregates: Some(vec![oxyde_codec::Aggregate {
                op: oxyde_codec::AggregateOp::Sum,
                field: Some("amount".into()),
                alias: Some("unique_total".into()),
                distinct: Some(true),
            }]),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Sqlite).unwrap();
        assert!(
            sql.contains("SUM(DISTINCT"),
            "should have SUM(DISTINCT: {sql}"
        );
    }

    #[test]
    fn test_insert_with_text_array_column() {
        let mut col_types = HashMap::new();
        col_types.insert("tags".to_string(), "str[]".to_string());
        col_types.insert("name".to_string(), "str".to_string());

        let ir = QueryIR {
            op: Operation::Insert,
            table: "documents".into(),
            values: Some(HashMap::from([
                ("name".into(), rmpv_str("doc1")),
                (
                    "tags".into(),
                    rmpv_arr(vec![rmpv_str("rust"), rmpv_str("async")]),
                ),
            ])),
            col_types: Some(col_types),
            returning: Some(true),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.contains("INSERT INTO"), "SQL: {sql}");
        assert!(sql.contains("RETURNING"), "SQL: {sql}");
        // Should have 2 params: name (string) and tags (array)
        assert_eq!(params.len(), 2, "params: {params:?}");
        // Verify the array param is Value::Array, not Value::String
        let array_param = params
            .iter()
            .find(|p| matches!(p, Value::Array(_, _)))
            .expect("should have an array parameter");
        match array_param {
            Value::Array(sea_query::ArrayType::String, Some(elems)) => {
                assert_eq!(elems.len(), 2);
            }
            other => panic!("expected Array(String, ...), got: {other:?}"),
        }
    }

    #[test]
    fn test_insert_with_uuid_array_column() {
        let mut col_types = HashMap::new();
        col_types.insert("tag_ids".to_string(), "uuid[]".to_string());

        let ir = QueryIR {
            op: Operation::Insert,
            table: "documents".into(),
            values: Some(HashMap::from([(
                "tag_ids".into(),
                rmpv_arr(vec![
                    rmpv_str("550e8400-e29b-41d4-a716-446655440000"),
                    rmpv_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8"),
                ]),
            )])),
            col_types: Some(col_types),
            ..Default::default()
        };
        let (_, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert_eq!(params.len(), 1);
        match &params[0] {
            Value::Array(sea_query::ArrayType::Uuid, Some(elems)) => {
                assert_eq!(elems.len(), 2);
                assert!(matches!(&elems[0], Value::Uuid(Some(_))));
            }
            other => panic!("expected Array(Uuid, ...), got: {other:?}"),
        }
    }

    #[test]
    fn test_update_with_array_column() {
        let mut col_types = HashMap::new();
        col_types.insert("tags".to_string(), "str[]".to_string());

        let ir = QueryIR {
            op: Operation::Update,
            table: "documents".into(),
            values: Some(HashMap::from([(
                "tags".into(),
                rmpv_arr(vec![rmpv_str("updated")]),
            )])),
            col_types: Some(col_types),
            filter_tree: Some(filter_cond("id", "=", rmpv_int(1))),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.contains("UPDATE"), "SQL: {sql}");
        assert_eq!(params.len(), 2); // array param + filter param
        let has_array = params.iter().any(|p| matches!(p, Value::Array(_, _)));
        assert!(has_array, "should have array param: {params:?}");
    }

    #[test]
    fn test_update_with_jsonb_map_value() {
        let mut col_types = HashMap::new();
        col_types.insert("id".to_string(), "uuid".to_string());
        col_types.insert("xattrs".to_string(), "JSONB".to_string());

        let map_val = rmpv::Value::Map(vec![(
            rmpv::Value::String("key".into()),
            rmpv::Value::String("value".into()),
        )]);

        let ir = QueryIR {
            op: Operation::Update,
            table: "documents".into(),
            values: Some(HashMap::from([("xattrs".into(), map_val)])),
            col_types: Some(col_types),
            filter_tree: Some(filter_cond(
                "id",
                "=",
                rmpv_str("550e8400-e29b-41d4-a716-446655440000"),
            )),
            ..Default::default()
        };
        let (sql, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(sql.contains("UPDATE"), "SQL: {sql}");
        assert_eq!(params.len(), 2, "params: {params:?}");
        // SET value should be Json, not String
        let has_json = params.iter().any(|p| matches!(p, Value::Json(_)));
        assert!(has_json, "should have Json param for xattrs: {params:?}");
        // WHERE value should be Uuid, not String
        let has_uuid = params.iter().any(|p| matches!(p, Value::Uuid(_)));
        assert!(has_uuid, "should have Uuid param for id filter: {params:?}");
    }

    #[test]
    fn test_insert_with_empty_array() {
        let mut col_types = HashMap::new();
        col_types.insert("tags".to_string(), "str[]".to_string());

        let ir = QueryIR {
            op: Operation::Insert,
            table: "documents".into(),
            values: Some(HashMap::from([("tags".into(), rmpv_arr(vec![]))])),
            col_types: Some(col_types),
            ..Default::default()
        };
        let (_, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert_eq!(params.len(), 1);
        match &params[0] {
            Value::Array(sea_query::ArrayType::String, Some(elems)) => {
                assert!(elems.is_empty());
            }
            other => panic!("expected empty Array, got: {other:?}"),
        }
    }

    #[test]
    fn test_insert_array_without_col_type_falls_back() {
        // Without col_types, arrays should fall back to string (existing behavior)
        let ir = QueryIR {
            op: Operation::Insert,
            table: "documents".into(),
            values: Some(HashMap::from([(
                "tags".into(),
                rmpv_arr(vec![rmpv_str("a"), rmpv_str("b")]),
            )])),
            // No col_types
            ..Default::default()
        };
        let (_, params) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert_eq!(params.len(), 1);
        // Should be a plain string, not an array
        assert!(
            matches!(&params[0], Value::String(Some(_))),
            "without col_types, array should serialize as string: {:?}",
            params[0]
        );
    }

    #[test]
    fn test_count_without_distinct() {
        let ir = QueryIR {
            table: "users".into(),
            aggregates: Some(vec![oxyde_codec::Aggregate {
                op: oxyde_codec::AggregateOp::Count,
                field: Some("email".into()),
                alias: Some("total".into()),
                distinct: None,
            }]),
            ..Default::default()
        };
        let (sql, _) = build_sql(&ir, Dialect::Postgres).unwrap();
        assert!(!sql.contains("DISTINCT"), "should NOT have DISTINCT: {sql}");
    }
}

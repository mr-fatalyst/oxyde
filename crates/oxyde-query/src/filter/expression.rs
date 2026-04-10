//! Filter expression building for WHERE clauses

use std::collections::HashMap;

use oxyde_codec::{Aggregate, Filter, FilterNode};
use sea_query::{Expr, Func, LikeExpr, SimpleExpr, Value};

use crate::aggregate::build_aggregate;
use crate::error::{QueryError, Result};
use crate::utils::{rmpv_to_value_typed, ColumnIdent, TableIdent};

/// Create column expression, handling "table.column" format for joins
/// If default_table is provided and column is not already qualified, prepend it
fn make_col_expr(col_name: &str, default_table: Option<&str>) -> Expr {
    if let Some((table, column)) = col_name.split_once('.') {
        // "user.age" -> ("user", "age") -> "user"."age"
        Expr::col((
            TableIdent(table.to_string()),
            ColumnIdent(column.to_string()),
        ))
    } else if let Some(table) = default_table {
        // Qualify with default table (for JOIN queries)
        Expr::col((
            TableIdent(table.to_string()),
            ColumnIdent(col_name.to_string()),
        ))
    } else {
        // Simple column name (no JOIN)
        Expr::col(ColumnIdent(col_name.to_string()))
    }
}

/// Resolve column type hint from col_types map.
/// Handles qualified names like "user.age" by extracting the column part.
fn resolve_col_type<'a>(
    col_name: &str,
    col_types: Option<&'a HashMap<String, String>>,
) -> Option<&'a str> {
    let col_key = col_name.rsplit('.').next().unwrap_or(col_name);
    col_types.and_then(|ct| ct.get(col_key).map(String::as_str))
}

fn like_expr(filter: &Filter, pattern: String) -> Result<LikeExpr> {
    let expr = LikeExpr::new(pattern);
    let Some(escape) = filter.escape.as_deref() else {
        return Ok(expr);
    };

    let mut chars = escape.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => Ok(expr.escape(ch)),
        _ => Err(QueryError::InvalidQuery(
            "LIKE escape requires a single character".into(),
        )),
    }
}

/// Build filter clause from FilterNode tree.
///
/// default_table: if provided, unqualified columns will be prefixed with this table name
/// col_types: type hints for typed value conversion in filter conditions
/// aggregates: if provided (HAVING), alias references are substituted with aggregate expressions
pub fn build_filter_node(
    node: &FilterNode,
    default_table: Option<&str>,
    col_types: Option<&HashMap<String, String>>,
    aggregates: Option<&[Aggregate]>,
) -> Result<SimpleExpr> {
    match node {
        FilterNode::Condition(filter) => apply_filter(filter, default_table, col_types, aggregates),
        FilterNode::And { conditions } => {
            if conditions.is_empty() {
                return Err(QueryError::InvalidQuery(
                    "AND node must have at least one condition".into(),
                ));
            }
            let first = build_filter_node(&conditions[0], default_table, col_types, aggregates)?;
            let mut result = first;
            for cond in &conditions[1..] {
                let next = build_filter_node(cond, default_table, col_types, aggregates)?;
                result = result.and(next);
            }
            Ok(result)
        }
        FilterNode::Or { conditions } => {
            if conditions.is_empty() {
                return Err(QueryError::InvalidQuery(
                    "OR node must have at least one condition".into(),
                ));
            }
            let first = build_filter_node(&conditions[0], default_table, col_types, aggregates)?;
            let mut result = first;
            for cond in &conditions[1..] {
                let next = build_filter_node(cond, default_table, col_types, aggregates)?;
                result = result.or(next);
            }
            Ok(result)
        }
        FilterNode::Not { condition } => {
            let inner = build_filter_node(condition, default_table, col_types, aggregates)?;
            Ok(inner.not())
        }
    }
}

/// Apply filter to expression.
/// If `aggregates` is provided and the field matches an aggregate alias,
/// the aggregate expression (e.g. SUM("views")) is used instead of a column ref.
fn apply_filter(
    filter: &Filter,
    default_table: Option<&str>,
    col_types: Option<&HashMap<String, String>>,
    aggregates: Option<&[Aggregate]>,
) -> Result<SimpleExpr> {
    let col_name = filter.column.as_ref().unwrap_or(&filter.field);

    // HAVING: substitute aggregate alias with aggregate expression
    let agg_match =
        aggregates.and_then(|aggs| aggs.iter().find(|a| a.alias.as_deref() == Some(col_name)));

    let col = if let Some(agg) = agg_match {
        Expr::expr(build_aggregate(agg)?)
    } else {
        make_col_expr(col_name, default_table)
    };

    let col_type = resolve_col_type(col_name, col_types);
    let val = rmpv_to_value_typed(&filter.value, col_type);

    let expr = match filter.operator.as_str() {
        "=" => col.eq(val),
        "!=" => col.ne(val),
        ">" => col.gt(val),
        ">=" => col.gte(val),
        "<" => col.lt(val),
        "<=" => col.lte(val),
        "LIKE" => {
            let text = filter.value.as_str().ok_or_else(|| {
                QueryError::InvalidQuery("LIKE operator requires string value".into())
            })?;
            col.like(like_expr(filter, text.to_string())?)
        }
        "ILIKE" => {
            let text = filter.value.as_str().ok_or_else(|| {
                QueryError::InvalidQuery("ILIKE operator requires string value".into())
            })?;
            let lowered = text.to_lowercase();
            let lower_col = Func::lower(make_col_expr(col_name, default_table));
            Expr::expr(lower_col).like(like_expr(filter, lowered)?)
        }
        "IN" => {
            if let rmpv::Value::Array(arr) = &filter.value {
                let values: Vec<Value> = arr
                    .iter()
                    .map(|v| rmpv_to_value_typed(v, col_type))
                    .collect();
                col.is_in(values)
            } else {
                return Err(QueryError::InvalidQuery(
                    "IN operator requires array value".to_string(),
                ));
            }
        }
        "BETWEEN" => {
            if let rmpv::Value::Array(arr) = &filter.value {
                if arr.len() != 2 {
                    return Err(QueryError::InvalidQuery(
                        "BETWEEN operator requires exactly two values".to_string(),
                    ));
                }
                let start = Expr::val(rmpv_to_value_typed(&arr[0], col_type));
                let end = Expr::val(rmpv_to_value_typed(&arr[1], col_type));
                col.between(start, end)
            } else {
                return Err(QueryError::InvalidQuery(
                    "BETWEEN operator requires array value".to_string(),
                ));
            }
        }
        "IS NULL" => col.is_null(),
        "IS NOT NULL" => col.is_not_null(),
        op => {
            return Err(QueryError::UnsupportedOperation(format!(
                "Unsupported operator: {op}",
            )))
        }
    };

    Ok(expr)
}

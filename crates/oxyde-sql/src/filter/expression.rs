//! Filter expression building for WHERE clauses

use std::collections::HashMap;

use oxyde_codec::{Aggregate, ColumnTypeSpec, Filter, FilterNode};
use sea_query::{Expr, Func, LikeExpr, SimpleExpr};

use crate::aggregate::build_aggregate;
use crate::error::{QueryError, Result};
use crate::utils::{bind_value, typed_value_expr, ColumnIdent, TableIdent};
use crate::Dialect;

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
fn resolve_col_spec<'a>(
    col_name: &str,
    col_types: Option<&'a HashMap<String, ColumnTypeSpec>>,
) -> &'a ColumnTypeSpec {
    if let Some(spec) = col_types.and_then(|ct| ct.get(col_name)) {
        return spec;
    }
    let col_key = col_name.rsplit('.').next().unwrap_or(col_name);
    col_types
        .and_then(|ct| ct.get(col_key))
        .unwrap_or(&ColumnTypeSpec::Unknown)
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
    col_types: Option<&HashMap<String, ColumnTypeSpec>>,
    aggregates: Option<&[Aggregate]>,
    dialect: Dialect,
) -> Result<SimpleExpr> {
    match node {
        FilterNode::Condition(filter) => {
            apply_filter(filter, default_table, col_types, aggregates, dialect)
        }
        FilterNode::And { conditions } => {
            if conditions.is_empty() {
                return Err(QueryError::InvalidQuery(
                    "AND node must have at least one condition".into(),
                ));
            }
            let first = build_filter_node(
                &conditions[0],
                default_table,
                col_types,
                aggregates,
                dialect,
            )?;
            let mut result = first;
            for cond in &conditions[1..] {
                let next = build_filter_node(cond, default_table, col_types, aggregates, dialect)?;
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
            let first = build_filter_node(
                &conditions[0],
                default_table,
                col_types,
                aggregates,
                dialect,
            )?;
            let mut result = first;
            for cond in &conditions[1..] {
                let next = build_filter_node(cond, default_table, col_types, aggregates, dialect)?;
                result = result.or(next);
            }
            Ok(result)
        }
        FilterNode::Not { condition } => {
            let inner =
                build_filter_node(condition, default_table, col_types, aggregates, dialect)?;
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
    col_types: Option<&HashMap<String, ColumnTypeSpec>>,
    aggregates: Option<&[Aggregate]>,
    dialect: Dialect,
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

    let spec = resolve_col_spec(col_name, col_types);
    let val = bind_value(&filter.value, spec);

    let expr = match filter.operator.as_str() {
        "=" => col.eq(typed_value_expr(val, spec, dialect)),
        "!=" => col.ne(typed_value_expr(val, spec, dialect)),
        ">" => col.gt(typed_value_expr(val, spec, dialect)),
        ">=" => col.gte(typed_value_expr(val, spec, dialect)),
        "<" => col.lt(typed_value_expr(val, spec, dialect)),
        "<=" => col.lte(typed_value_expr(val, spec, dialect)),
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
                let values: Vec<SimpleExpr> = arr
                    .iter()
                    .map(|v| typed_value_expr(bind_value(v, spec), spec, dialect))
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
                let start = typed_value_expr(bind_value(&arr[0], spec), spec, dialect);
                let end = typed_value_expr(bind_value(&arr[1], spec), spec, dialect);
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

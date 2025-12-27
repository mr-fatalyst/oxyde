//! Filter expression building for WHERE clauses

use oxyde_codec::{Filter, FilterNode};
use sea_query::{BinOper, Expr, Func, SimpleExpr, Value};

use crate::error::{QueryError, Result};
use crate::utils::{json_to_value, ColumnIdent, TableIdent};

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

/// Build WHERE clause from FilterNode tree
/// default_table: if provided, unqualified columns will be prefixed with this table name
pub fn build_filter_node(node: &FilterNode, default_table: Option<&str>) -> Result<SimpleExpr> {
    match node {
        FilterNode::Condition(filter) => apply_filter(filter, default_table),
        FilterNode::And { conditions } => {
            if conditions.is_empty() {
                return Err(QueryError::InvalidQuery(
                    "AND node must have at least one condition".into(),
                ));
            }
            let first = build_filter_node(&conditions[0], default_table)?;
            let mut result = first;
            for cond in &conditions[1..] {
                let next = build_filter_node(cond, default_table)?;
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
            let first = build_filter_node(&conditions[0], default_table)?;
            let mut result = first;
            for cond in &conditions[1..] {
                let next = build_filter_node(cond, default_table)?;
                result = result.or(next);
            }
            Ok(result)
        }
        FilterNode::Not { condition } => {
            let inner = build_filter_node(condition, default_table)?;
            Ok(inner.not())
        }
    }
}

/// Apply filter to expression
pub fn apply_filter(filter: &Filter, default_table: Option<&str>) -> Result<SimpleExpr> {
    let col_name = filter.column.as_ref().unwrap_or(&filter.field);
    let col = make_col_expr(col_name, default_table);
    let val = json_to_value(&filter.value);

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
            col.binary(BinOper::Like, Expr::val(Value::from(text.to_string())))
        }
        "ILIKE" => {
            let text = filter.value.as_str().ok_or_else(|| {
                QueryError::InvalidQuery("ILIKE operator requires string value".into())
            })?;
            let lowered = text.to_lowercase();
            // Use col_name (respects filter.column alias) instead of filter.field
            let lower_col = Func::lower(make_col_expr(col_name, default_table));
            Expr::expr(lower_col).binary(BinOper::Like, Expr::val(Value::from(lowered)))
        }
        "IN" => {
            // For IN operator, value should be an array
            if let serde_json::Value::Array(arr) = &filter.value {
                let values: Vec<Value> = arr.iter().map(json_to_value).collect();
                col.is_in(values)
            } else {
                return Err(QueryError::InvalidQuery(
                    "IN operator requires array value".to_string(),
                ));
            }
        }
        "BETWEEN" => {
            if let serde_json::Value::Array(arr) = &filter.value {
                if arr.len() != 2 {
                    return Err(QueryError::InvalidQuery(
                        "BETWEEN operator requires exactly two values".to_string(),
                    ));
                }
                let start = Expr::val(json_to_value(&arr[0]));
                let end = Expr::val(json_to_value(&arr[1]));
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
                "Unsupported operator: {}",
                op
            )))
        }
    };

    Ok(expr)
}

//! JSON to sea_query Value conversion utilities

use sea_query::{Expr, SimpleExpr, Value};

use crate::error::{QueryError, Result};
use crate::utils::identifier::ColumnIdent;

/// Convert JSON value to sea_query Value
pub fn json_to_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::String(None),
        serde_json::Value::Bool(b) => Value::Bool(Some(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::BigInt(Some(i))
            } else if let Some(f) = n.as_f64() {
                Value::Double(Some(f))
            } else {
                Value::String(Some(Box::new(n.to_string())))
            }
        }
        serde_json::Value::String(s) => Value::String(Some(Box::new(s.clone()))),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Value::String(Some(Box::new(value.to_string())))
        }
    }
}

/// Convert JSON value to SimpleExpr if it contains an expression
pub fn json_to_simple_expr(value: &serde_json::Value) -> Result<Option<SimpleExpr>> {
    if let Some(expr) = value.get("__expr__") {
        return Ok(Some(parse_expression(expr)?));
    }
    Ok(None)
}

/// Parse expression node from JSON
pub fn parse_expression(node: &serde_json::Value) -> Result<SimpleExpr> {
    let obj = node
        .as_object()
        .ok_or_else(|| QueryError::InvalidQuery("Expression node must be an object".into()))?;
    let expr_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| QueryError::InvalidQuery("Expression node missing type".into()))?;
    match expr_type {
        "value" => {
            Ok(Expr::val(json_to_value(obj.get("value").ok_or_else(|| {
                QueryError::InvalidQuery("Value node missing 'value'".into())
            })?))
            .into())
        }
        "column" => {
            let name = obj
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| QueryError::InvalidQuery("Column node missing 'name'".into()))?;
            Ok(Expr::col(ColumnIdent(name.to_string())).into())
        }
        "op" => {
            let op = obj
                .get("op")
                .and_then(|v| v.as_str())
                .ok_or_else(|| QueryError::InvalidQuery("Operator node missing 'op'".into()))?;
            let lhs =
                parse_expression(obj.get("lhs").ok_or_else(|| {
                    QueryError::InvalidQuery("Operator node missing 'lhs'".into())
                })?)?;
            let rhs =
                parse_expression(obj.get("rhs").ok_or_else(|| {
                    QueryError::InvalidQuery("Operator node missing 'rhs'".into())
                })?)?;
            let expr = match op {
                "add" => Expr::expr(lhs).add(rhs),
                "sub" => Expr::expr(lhs).sub(rhs),
                "mul" => Expr::expr(lhs).mul(rhs),
                "div" => Expr::expr(lhs).div(rhs),
                other => {
                    return Err(QueryError::InvalidQuery(format!(
                        "Unsupported arithmetic operator '{}'",
                        other
                    )))
                }
            };
            Ok(expr)
        }
        "neg" => {
            let inner =
                parse_expression(obj.get("expr").ok_or_else(|| {
                    QueryError::InvalidQuery("Negation node missing 'expr'".into())
                })?)?;
            Ok(Expr::val(Value::BigInt(Some(0))).sub(inner))
        }
        other => Err(QueryError::InvalidQuery(format!(
            "Unsupported expression node type '{}'",
            other
        ))),
    }
}

//! F-expression parsing (`__expr__` IR nodes) and rmpv→JSON conversion.
//!
//! Value binding lives in `bind.rs` (`bind_value`, spec-driven). This module
//! parses arithmetic expression trees produced by Python's `F()` and converts
//! rmpv maps/arrays into `serde_json::Value` for JSON column storage.

use sea_query::{Expr, SimpleExpr, Value};

use crate::error::{QueryError, Result};
use crate::utils::identifier::ColumnIdent;
use oxyde_codec::ColumnTypeSpec;

/// Convert rmpv::Value to serde_json::Value for JSON column storage.
/// Returns `None` for values that cannot be represented in JSON (NaN, Ext).
pub(crate) fn rmpv_to_json(value: &rmpv::Value) -> Option<serde_json::Value> {
    Some(match value {
        rmpv::Value::Nil => serde_json::Value::Null,
        rmpv::Value::Boolean(b) => serde_json::Value::Bool(*b),
        rmpv::Value::Integer(n) => {
            let num = n
                .as_i64()
                .map(serde_json::Number::from)
                .or_else(|| n.as_u64().map(serde_json::Number::from))?;
            serde_json::Value::Number(num)
        }
        rmpv::Value::F32(f) => {
            serde_json::Value::Number(serde_json::Number::from_f64(f64::from(*f))?)
        }
        rmpv::Value::F64(f) => serde_json::Value::Number(serde_json::Number::from_f64(*f)?),
        rmpv::Value::String(s) => {
            serde_json::Value::String(s.as_str().unwrap_or_default().to_string())
        }
        rmpv::Value::Binary(b) => serde_json::Value::Array(
            b.iter()
                .map(|&byte| serde_json::Value::Number(byte.into()))
                .collect(),
        ),
        rmpv::Value::Array(arr) => {
            let items: Option<Vec<_>> = arr.iter().map(rmpv_to_json).collect();
            serde_json::Value::Array(items?)
        }
        rmpv::Value::Map(pairs) => {
            let mut map = serde_json::Map::new();
            for (k, v) in pairs {
                let key = k
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| k.to_string());
                map.insert(key, rmpv_to_json(v)?);
            }
            serde_json::Value::Object(map)
        }
        rmpv::Value::Ext(_, _) => return None,
    })
}

/// Convert rmpv value to SimpleExpr if it contains an expression
pub fn rmpv_to_simple_expr(value: &rmpv::Value) -> Result<Option<SimpleExpr>> {
    if let Some(expr) = map_get(value, "__expr__") {
        return Ok(Some(parse_expression(expr)?));
    }
    Ok(None)
}

/// Parse expression node from rmpv map
pub fn parse_expression(node: &rmpv::Value) -> Result<SimpleExpr> {
    let expr_type = map_get_str(node, "type")
        .ok_or_else(|| QueryError::InvalidQuery("Expression node missing type".into()))?;

    match expr_type {
        "value" => {
            let val = map_get(node, "value")
                .ok_or_else(|| QueryError::InvalidQuery("Value node missing 'value'".into()))?;
            // Optional per-literal ColumnTypeSpec hint (tagged dict), set by
            // Python for types that msgpack-encode as strings (Decimal, UUID,
            // datetime, ...). Absent hint = native conversion.
            let spec = map_get(node, "value_type")
                .and_then(|v| rmpv::ext::from_value::<ColumnTypeSpec>(v.clone()).ok())
                .unwrap_or(ColumnTypeSpec::Unknown);
            Ok(Expr::val(super::bind::bind_value(val, &spec)).into())
        }
        "column" => {
            let name = map_get_str(node, "name")
                .ok_or_else(|| QueryError::InvalidQuery("Column node missing 'name'".into()))?;
            Ok(Expr::col(ColumnIdent(name.to_string())).into())
        }
        "op" => {
            let op = map_get_str(node, "op")
                .ok_or_else(|| QueryError::InvalidQuery("Operator node missing 'op'".into()))?;
            let lhs =
                parse_expression(map_get(node, "lhs").ok_or_else(|| {
                    QueryError::InvalidQuery("Operator node missing 'lhs'".into())
                })?)?;
            let rhs =
                parse_expression(map_get(node, "rhs").ok_or_else(|| {
                    QueryError::InvalidQuery("Operator node missing 'rhs'".into())
                })?)?;
            let expr = match op {
                "add" => Expr::expr(lhs).add(rhs),
                "sub" => Expr::expr(lhs).sub(rhs),
                "mul" => Expr::expr(lhs).mul(rhs),
                "div" => Expr::expr(lhs).div(rhs),
                other => {
                    return Err(QueryError::InvalidQuery(format!(
                        "Unsupported arithmetic operator '{other}'"
                    )))
                }
            };
            Ok(expr)
        }
        "neg" => {
            let inner =
                parse_expression(map_get(node, "expr").ok_or_else(|| {
                    QueryError::InvalidQuery("Negation node missing 'expr'".into())
                })?)?;
            Ok(Expr::val(Value::BigInt(Some(0))).sub(inner))
        }
        other => Err(QueryError::InvalidQuery(format!(
            "Unsupported expression node type '{other}'"
        ))),
    }
}

// ── rmpv map helpers ──────────────────────────────────────────────────

/// Get a value from an rmpv Map by string key.
fn map_get<'a>(map: &'a rmpv::Value, key: &str) -> Option<&'a rmpv::Value> {
    match map {
        rmpv::Value::Map(pairs) => pairs.iter().find_map(|(k, v)| {
            if k.as_str() == Some(key) {
                Some(v)
            } else {
                None
            }
        }),
        _ => None,
    }
}

/// Get a string value from an rmpv Map by key.
fn map_get_str<'a>(map: &'a rmpv::Value, key: &str) -> Option<&'a str> {
    map_get(map, key).and_then(|v| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn test_rmpv_to_json_nested() {
        let val = rmpv::Value::Map(vec![
            (
                rmpv::Value::String("items".into()),
                rmpv::Value::Array(vec![
                    rmpv::Value::Integer(1.into()),
                    rmpv::Value::Boolean(true),
                    rmpv::Value::Nil,
                ]),
            ),
            (
                rmpv::Value::String("count".into()),
                rmpv::Value::Integer(3.into()),
            ),
        ]);
        let json = rmpv_to_json(&val).unwrap();
        assert_eq!(json["count"], 3);
        assert_eq!(json["items"][0], 1);
        assert_eq!(json["items"][1], true);
        assert!(json["items"][2].is_null());
    }

    // ── parse_expression: value_type spec on literal nodes ──────────────

    /// Build a `{"type": "value", "value": <val>, "value_type": {"kind": <kind>}}`
    /// rmpv map — the shape Python sends for F-expression literals.
    /// If `kind` is None, the key is omitted (no hint = native conversion).
    fn build_value_node(value: rmpv::Value, kind: Option<&str>) -> rmpv::Value {
        let mut pairs = vec![
            (
                rmpv::Value::String("type".into()),
                rmpv::Value::String("value".into()),
            ),
            (rmpv::Value::String("value".into()), value),
        ];
        if let Some(k) = kind {
            pairs.push((
                rmpv::Value::String("value_type".into()),
                rmpv::Value::Map(vec![(
                    rmpv::Value::String("kind".into()),
                    rmpv::Value::String(k.into()),
                )]),
            ));
        }
        rmpv::Value::Map(pairs)
    }

    #[test]
    fn test_parse_expression_value_with_decimal_hint() {
        let node = build_value_node(rmpv::Value::String("2.50".into()), Some("decimal"));
        let expr = parse_expression(&node).unwrap();
        match expr {
            SimpleExpr::Value(Value::Decimal(Some(d))) => {
                assert_eq!(*d, Decimal::from_str("2.50").unwrap());
            }
            other => panic!("expected Value::Decimal, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_expression_value_without_hint_stays_string() {
        // No value_type → native conversion: Decimal-looking strings bind
        // as String, exactly like any untyped string parameter.
        let node = build_value_node(rmpv::Value::String("2.50".into()), None);
        let expr = parse_expression(&node).unwrap();
        match expr {
            SimpleExpr::Value(Value::String(Some(s))) => {
                assert_eq!(s.as_ref(), "2.50");
            }
            other => panic!("expected Value::String without hint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_expression_value_with_uuid_hint() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let node = build_value_node(rmpv::Value::String(uuid_str.into()), Some("uuid"));
        let expr = parse_expression(&node).unwrap();
        match expr {
            SimpleExpr::Value(Value::Uuid(Some(u))) => {
                assert_eq!(u.to_string(), uuid_str);
            }
            other => panic!("expected Value::Uuid, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_expression_op_with_decimal_inside() {
        // Nested: (column "price") + (value "2.50" with decimal spec)
        // The literal deep inside the op tree must still bind as Decimal.
        let lhs = rmpv::Value::Map(vec![
            (
                rmpv::Value::String("type".into()),
                rmpv::Value::String("column".into()),
            ),
            (
                rmpv::Value::String("name".into()),
                rmpv::Value::String("price".into()),
            ),
        ]);
        let rhs = build_value_node(rmpv::Value::String("2.50".into()), Some("decimal"));
        let op_node = rmpv::Value::Map(vec![
            (
                rmpv::Value::String("type".into()),
                rmpv::Value::String("op".into()),
            ),
            (
                rmpv::Value::String("op".into()),
                rmpv::Value::String("add".into()),
            ),
            (rmpv::Value::String("lhs".into()), lhs),
            (rmpv::Value::String("rhs".into()), rhs),
        ]);
        let expr = parse_expression(&op_node).unwrap();
        let SimpleExpr::Binary(_, _, rhs_box) = expr else {
            panic!("expected SimpleExpr::Binary, got {expr:?}");
        };
        match *rhs_box {
            SimpleExpr::Value(Value::Decimal(Some(d))) => {
                assert_eq!(*d, Decimal::from_str("2.50").unwrap());
            }
            other => panic!("expected nested Value::Decimal on RHS, got {other:?}"),
        }
    }
}

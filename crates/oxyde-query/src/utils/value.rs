//! rmpv::Value to sea_query Value conversion utilities

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use sea_query::{Expr, SimpleExpr, Value};
use uuid::Uuid;

use crate::error::{QueryError, Result};
use crate::utils::identifier::ColumnIdent;

/// Convert rmpv value to sea_query Value without type hint (legacy behavior)
pub fn rmpv_to_value(value: &rmpv::Value) -> Value {
    rmpv_to_value_typed(value, None)
}

/// Convert rmpv value to sea_query Value with optional type hint from col_types.
///
/// Type hints come from Python model metadata, either as IR type names
/// ("int", "uuid", "json", "decimal") or SQL type names ("BIGSERIAL", "JSONB",
/// "NUMERIC(10,2)"). Both forms are handled via uppercase normalization.
///
/// When `col_type` is provided:
/// - Nil values produce typed nulls (e.g., `Value::Uuid(None)` instead of `Value::String(None)`)
/// - String values are parsed into native types (UUID, Decimal, JSON, datetime)
/// - Map values with JSON type hint are converted to `Value::Json`
pub fn rmpv_to_value_typed(value: &rmpv::Value, col_type: Option<&str>) -> Value {
    // Uppercase once, reuse for all matching
    let typ = col_type.map(|t| t.to_uppercase());
    let typ_str = typ.as_deref();

    match value {
        rmpv::Value::Nil => typed_null(typ_str),
        rmpv::Value::Boolean(b) => Value::Bool(Some(*b)),
        rmpv::Value::Integer(n) => {
            if let Some(i) = n.as_i64() {
                Value::BigInt(Some(i))
            } else if let Some(u) = n.as_u64() {
                // u64 that doesn't fit i64 — store as string
                Value::String(Some(Box::new(u.to_string())))
            } else {
                Value::String(Some(Box::new(n.to_string())))
            }
        }
        rmpv::Value::F32(f) => Value::Double(Some(f64::from(*f))),
        rmpv::Value::F64(f) => Value::Double(Some(*f)),
        rmpv::Value::String(s) => {
            let s = s.as_str().unwrap_or_default();
            if let Some(t) = typ_str {
                if let Some(v) = parse_string_by_type(s, t) {
                    return v;
                }
            }
            // RFC3339 is strict enough to try without type hint
            if let Some(dt) = parse_datetime_utc(s) {
                return Value::ChronoDateTimeUtc(Some(Box::new(dt)));
            }
            Value::String(Some(Box::new(s.to_string())))
        }
        rmpv::Value::Binary(b) => Value::Bytes(Some(Box::new(b.clone()))),
        rmpv::Value::Map(_) => {
            if matches!(typ_str, Some("JSON" | "JSONB")) {
                if let Some(j) = rmpv_to_json(value) {
                    return Value::Json(Some(Box::new(j)));
                }
            }
            Value::String(Some(Box::new(format!("{value}"))))
        }
        rmpv::Value::Array(_) => {
            if matches!(typ_str, Some("JSON" | "JSONB")) {
                if let Some(j) = rmpv_to_json(value) {
                    return Value::Json(Some(Box::new(j)));
                }
            }
            Value::String(Some(Box::new(format!("{value}"))))
        }
        rmpv::Value::Ext(_, _) => Value::String(Some(Box::new(format!("{value}")))),
    }
}

/// Parse a string value into a typed sea_query Value based on col_type hint.
/// Returns `None` if parsing fails — caller falls through to default behavior.
fn parse_string_by_type(s: &str, typ: &str) -> Option<Value> {
    match typ {
        "DATETIME" | "TIMESTAMP" | "TIMESTAMPTZ" => parse_datetime(s)
            .ok()
            .map(|dt| Value::ChronoDateTime(Some(Box::new(dt)))),
        "DATE" => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .ok()
            .map(|d| Value::ChronoDate(Some(Box::new(d)))),
        "TIME" | "TIMETZ" => parse_time(s)
            .ok()
            .map(|t| Value::ChronoTime(Some(Box::new(t)))),
        "UUID" => Uuid::parse_str(s)
            .ok()
            .map(|u| Value::Uuid(Some(Box::new(u)))),
        "JSON" | "JSONB" => serde_json::from_str(s)
            .ok()
            .map(|j| Value::Json(Some(Box::new(j)))),
        t if is_decimal_type(t) => s
            .parse::<Decimal>()
            .ok()
            .map(|d| Value::Decimal(Some(Box::new(d)))),
        _ => None,
    }
}

/// Produce a typed NULL value based on the column type hint.
/// Without a type hint, defaults to `Value::String(None)`.
fn typed_null(typ: Option<&str>) -> Value {
    match typ {
        Some(
            "INT" | "INTEGER" | "BIGINT" | "SMALLINT" | "TINYINT" | "SERIAL" | "BIGSERIAL"
            | "SMALLSERIAL" | "INT2" | "INT4" | "INT8" | "TIMEDELTA" | "INTERVAL",
        ) => Value::BigInt(None),
        Some("FLOAT" | "DOUBLE" | "REAL" | "FLOAT4" | "FLOAT8" | "DOUBLE PRECISION") => {
            Value::Double(None)
        }
        Some("BOOL" | "BOOLEAN") => Value::Bool(None),
        Some("UUID") => Value::Uuid(None),
        Some("JSON" | "JSONB") => Value::Json(None),
        Some("DATETIME" | "TIMESTAMP" | "TIMESTAMPTZ") => Value::ChronoDateTime(None),
        Some("DATE") => Value::ChronoDate(None),
        Some("TIME" | "TIMETZ") => Value::ChronoTime(None),
        Some("BYTEA" | "BLOB" | "BYTES") => Value::Bytes(None),
        Some(t) if is_decimal_type(t) => Value::Decimal(None),
        _ => Value::String(None),
    }
}

/// Check if SQL type name represents a decimal/numeric type.
/// Handles "DECIMAL", "NUMERIC", "DECIMAL(10,2)", "NUMERIC(5,3)", etc.
fn is_decimal_type(t: &str) -> bool {
    t.starts_with("DECIMAL") || t.starts_with("NUMERIC")
}

/// Convert rmpv::Value to serde_json::Value for JSON column storage.
/// Returns `None` for values that cannot be represented in JSON (NaN, Ext).
fn rmpv_to_json(value: &rmpv::Value) -> Option<serde_json::Value> {
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

/// Parse tz-aware datetime string and normalize to UTC
fn parse_datetime_utc(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

/// Parse datetime string in various ISO-like formats
fn parse_datetime(s: &str) -> std::result::Result<NaiveDateTime, chrono::ParseError> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f"))
}

/// Parse time string in various formats
fn parse_time(s: &str) -> std::result::Result<NaiveTime, chrono::ParseError> {
    NaiveTime::parse_from_str(s, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S%.f"))
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
            Ok(Expr::val(rmpv_to_value(val)).into())
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
    use std::str::FromStr;

    #[test]
    fn test_uuid_string_with_type_hint() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let val = rmpv::Value::String(uuid_str.into());
        let result = rmpv_to_value_typed(&val, Some("uuid"));
        match result {
            Value::Uuid(Some(u)) => assert_eq!(u.to_string(), uuid_str),
            other => panic!("expected Value::Uuid, got {other:?}"),
        }
    }

    #[test]
    fn test_uuid_string_with_sql_type() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let val = rmpv::Value::String(uuid_str.into());
        let result = rmpv_to_value_typed(&val, Some("UUID"));
        assert!(matches!(result, Value::Uuid(Some(_))));
    }

    #[test]
    fn test_uuid_invalid_fallback() {
        let val = rmpv::Value::String("not-a-uuid".into());
        let result = rmpv_to_value_typed(&val, Some("uuid"));
        assert!(matches!(result, Value::String(Some(_))));
    }

    #[test]
    fn test_decimal_string_with_type_hint() {
        let val = rmpv::Value::String("99.99".into());
        let result = rmpv_to_value_typed(&val, Some("decimal"));
        match result {
            Value::Decimal(Some(d)) => assert_eq!(*d, Decimal::from_str("99.99").unwrap()),
            other => panic!("expected Value::Decimal, got {other:?}"),
        }
    }

    #[test]
    fn test_decimal_numeric_sql_type() {
        let val = rmpv::Value::String("123.456".into());
        let result = rmpv_to_value_typed(&val, Some("NUMERIC(10,3)"));
        assert!(matches!(result, Value::Decimal(Some(_))));
    }

    #[test]
    fn test_json_string_with_type_hint() {
        let val = rmpv::Value::String(r#"{"key": "value"}"#.into());
        let result = rmpv_to_value_typed(&val, Some("json"));
        match result {
            Value::Json(Some(j)) => assert_eq!(j["key"], "value"),
            other => panic!("expected Value::Json, got {other:?}"),
        }
    }

    #[test]
    fn test_json_map_with_type_hint() {
        let val = rmpv::Value::Map(vec![(
            rmpv::Value::String("key".into()),
            rmpv::Value::String("value".into()),
        )]);
        let result = rmpv_to_value_typed(&val, Some("jsonb"));
        match result {
            Value::Json(Some(j)) => assert_eq!(j["key"], "value"),
            other => panic!("expected Value::Json, got {other:?}"),
        }
    }

    #[test]
    fn test_json_array_with_type_hint() {
        let val = rmpv::Value::Array(vec![
            rmpv::Value::Integer(1.into()),
            rmpv::Value::Integer(2.into()),
        ]);
        let result = rmpv_to_value_typed(&val, Some("json"));
        match result {
            Value::Json(Some(j)) => {
                let arr = j.as_array().unwrap();
                assert_eq!(arr.len(), 2);
                assert_eq!(arr[0], 1);
            }
            other => panic!("expected Value::Json, got {other:?}"),
        }
    }

    #[test]
    fn test_map_without_json_hint_fallback() {
        let val = rmpv::Value::Map(vec![(
            rmpv::Value::String("key".into()),
            rmpv::Value::String("value".into()),
        )]);
        let result = rmpv_to_value_typed(&val, None);
        assert!(matches!(result, Value::String(Some(_))));
    }

    #[test]
    fn test_typed_null_int() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("int")),
            Value::BigInt(None)
        ));
    }

    #[test]
    fn test_typed_null_bigserial() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("BIGSERIAL")),
            Value::BigInt(None)
        ));
    }

    #[test]
    fn test_typed_null_serial() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("SERIAL")),
            Value::BigInt(None)
        ));
    }

    #[test]
    fn test_typed_null_uuid() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("uuid")),
            Value::Uuid(None)
        ));
    }

    #[test]
    fn test_typed_null_json() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("jsonb")),
            Value::Json(None)
        ));
    }

    #[test]
    fn test_typed_null_decimal() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("NUMERIC(10,2)")),
            Value::Decimal(None)
        ));
    }

    #[test]
    fn test_typed_null_datetime() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("datetime")),
            Value::ChronoDateTime(None)
        ));
    }

    #[test]
    fn test_typed_null_bool() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("bool")),
            Value::Bool(None)
        ));
    }

    #[test]
    fn test_typed_null_float() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("DOUBLE PRECISION")),
            Value::Double(None)
        ));
    }

    #[test]
    fn test_typed_null_bytes() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("BYTEA")),
            Value::Bytes(None)
        ));
    }

    #[test]
    fn test_typed_null_no_hint() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, None),
            Value::String(None)
        ));
    }

    #[test]
    fn test_typed_null_unknown_type() {
        assert!(matches!(
            rmpv_to_value_typed(&rmpv::Value::Nil, Some("CUSTOM_TYPE")),
            Value::String(None)
        ));
    }

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

    #[test]
    fn test_datetime_with_hint_still_works() {
        let val = rmpv::Value::String("2024-01-15T10:30:00".into());
        let result = rmpv_to_value_typed(&val, Some("datetime"));
        assert!(matches!(result, Value::ChronoDateTime(Some(_))));
    }

    #[test]
    fn test_no_hint_preserves_legacy_behavior() {
        // Integer → BigInt
        let val = rmpv::Value::Integer(42.into());
        assert!(matches!(rmpv_to_value(&val), Value::BigInt(Some(42))));

        // String → String
        let val = rmpv::Value::String("hello".into());
        match rmpv_to_value(&val) {
            Value::String(Some(s)) => assert_eq!(s.as_ref(), "hello"),
            other => panic!("expected String, got {other:?}"),
        }

        // Bool → Bool
        let val = rmpv::Value::Boolean(true);
        assert!(matches!(rmpv_to_value(&val), Value::Bool(Some(true))));
    }
}

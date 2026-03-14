//! rmpv::Value to sea_query Value conversion utilities

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sea_query::{ArrayType, Expr, SimpleExpr, Value};

use crate::error::{QueryError, Result};
use crate::utils::identifier::ColumnIdent;

/// Convert rmpv value to sea_query Value without type hint (legacy behavior)
pub fn rmpv_to_value(value: &rmpv::Value) -> Value {
    rmpv_to_value_typed(value, None)
}

/// Convert rmpv value to sea_query Value with optional type hint from col_types.
///
/// When `col_type` is provided, parses string values into appropriate types:
/// - "datetime" -> Value::ChronoDateTime (parses ISO format string)
/// - "date" -> Value::ChronoDate
/// - "time" -> Value::ChronoTime
/// - "uuid" -> Value::Uuid (parsed from string, binds with correct PG OID 2950)
/// - "json"/"jsonb" -> Value::Json (binds with correct PG OID 3802)
/// - "decimal" -> Value::String (Decimal stored as string, DB handles it)
pub fn rmpv_to_value_typed(value: &rmpv::Value, col_type: Option<&str>) -> Value {
    match value {
        rmpv::Value::Nil => {
            // For typed columns, use the correct NULL variant so PostgreSQL
            // receives the right type OID (e.g., jsonb NULL, uuid NULL).
            if let Some(typ) = col_type {
                match typ.to_uppercase().as_str() {
                    "JSON" | "JSONB" => return Value::Json(None),
                    "UUID" => return Value::Uuid(None),
                    "BOOL" | "BOOLEAN" => return Value::Bool(None),
                    "INT" | "INT2" | "INT4" | "INT8" | "BIGINT" | "INTEGER" | "SMALLINT" => {
                        return Value::BigInt(None)
                    }
                    "FLOAT" | "FLOAT4" | "FLOAT8" | "REAL" | "DOUBLE PRECISION" => {
                        return Value::Double(None)
                    }
                    "DATETIME" | "TIMESTAMP" | "TIMESTAMPTZ" => {
                        return Value::ChronoDateTimeUtc(None)
                    }
                    "DATE" => return Value::ChronoDate(None),
                    "TIME" | "TIMETZ" => return Value::ChronoTime(None),
                    "BYTES" | "BYTEA" => return Value::Bytes(None),
                    s if s.ends_with("[]") => {
                        // NULL array: determine the array element type
                        let base = &s[..s.len() - 2];
                        let array_type = match base {
                            "UUID" => ArrayType::Uuid,
                            "INT" | "INT2" | "INT4" | "INT8" | "BIGINT" | "INTEGER"
                            | "SMALLINT" => ArrayType::BigInt,
                            "FLOAT" | "FLOAT4" | "FLOAT8" | "REAL" => ArrayType::Double,
                            "BOOL" | "BOOLEAN" => ArrayType::Bool,
                            "JSON" | "JSONB" => ArrayType::Json,
                            _ => ArrayType::String,
                        };
                        return Value::Array(array_type, None);
                    }
                    _ => {}
                }
            }
            Value::String(None)
        }
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
            // With type hint: try naive datetime/date/time formats
            if let Some(typ) = col_type {
                match typ.to_uppercase().as_str() {
                    "DATETIME" | "TIMESTAMP" | "TIMESTAMPTZ" => {
                        if let Ok(dt) = parse_datetime(s) {
                            return Value::ChronoDateTime(Some(Box::new(dt)));
                        }
                    }
                    "DATE" => {
                        if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                            return Value::ChronoDate(Some(Box::new(d)));
                        }
                    }
                    "TIME" => {
                        if let Ok(t) = parse_time(s) {
                            return Value::ChronoTime(Some(Box::new(t)));
                        }
                    }
                    "UUID" => {
                        if let Ok(u) = uuid::Uuid::parse_str(s) {
                            return Value::Uuid(Some(Box::new(u)));
                        }
                    }
                    "JSON" | "JSONB" => {
                        // String value for a JSON column — parse it as JSON
                        if let Ok(j) = serde_json::from_str::<serde_json::Value>(s) {
                            return Value::Json(Some(Box::new(j)));
                        }
                        // If it doesn't parse as JSON, wrap as JSON string
                        return Value::Json(Some(Box::new(serde_json::Value::String(
                            s.to_string(),
                        ))));
                    }
                    _ => {}
                }
            }
            // RFC3339 is strict enough to try without type hint
            if let Some(dt) = parse_datetime_utc(s) {
                return Value::ChronoDateTimeUtc(Some(Box::new(dt)));
            }
            Value::String(Some(Box::new(s.to_string())))
        }
        rmpv::Value::Binary(b) => {
            // Native binary — pass through as bytes
            Value::Bytes(Some(Box::new(b.clone())))
        }
        rmpv::Value::Array(arr) => {
            // If col_type indicates a PostgreSQL array (e.g. "str[]", "uuid[]", "int[]"),
            // convert to sea_query::Value::Array with properly typed elements.
            if let Some(base_type) = col_type.and_then(|ct| ct.strip_suffix("[]")) {
                let (array_type, elements) = convert_array_elements(arr, base_type);
                Value::Array(array_type, Some(Box::new(elements)))
            } else if col_type
                .map(|ct| ct.eq_ignore_ascii_case("json") || ct.eq_ignore_ascii_case("jsonb"))
                .unwrap_or(false)
            {
                // JSON/JSONB arrays: serialize as JSON value
                let json = rmpv_array_to_json(arr);
                Value::Json(Some(Box::new(json)))
            } else {
                // Fallback: serialize to JSON string
                Value::String(Some(Box::new(format!("{value}"))))
            }
        }
        rmpv::Value::Map(_) => {
            // Maps are always best represented as JSON (for JSONB columns or any map data)
            let json = rmpv_to_json(value);
            Value::Json(Some(Box::new(json)))
        }
        rmpv::Value::Ext(_, _) => {
            // Fallback: serialize to string
            Value::String(Some(Box::new(format!("{value}"))))
        }
    }
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

/// Convert an array of rmpv values to sea_query Values based on the base IR type.
fn convert_array_elements(arr: &[rmpv::Value], base_type: &str) -> (ArrayType, Vec<Value>) {
    // Normalize uppercase DB type names (e.g., "UUID" from "UUID[]") to lowercase IR names
    let normalized = match base_type.to_uppercase().as_str() {
        "TEXT" | "VARCHAR" | "CHAR" => "str",
        "INT2" | "INT4" | "INT8" | "BIGINT" | "INTEGER" | "SMALLINT" => "int",
        "FLOAT4" | "FLOAT8" | "REAL" | "DOUBLE PRECISION" => "float",
        "BOOL" | "BOOLEAN" => "bool",
        "TIMESTAMPTZ" | "TIMESTAMP" => "datetime",
        _ => base_type,
    };
    match normalized.to_lowercase().as_str() {
        "str" => (
            ArrayType::String,
            arr.iter()
                .map(|v| {
                    let s = match v {
                        rmpv::Value::String(s) => s.as_str().unwrap_or_default().to_string(),
                        rmpv::Value::Nil => return Value::String(None),
                        other => format!("{other}"),
                    };
                    Value::String(Some(Box::new(s)))
                })
                .collect(),
        ),
        "int" => (
            ArrayType::BigInt,
            arr.iter()
                .map(|v| match v {
                    rmpv::Value::Integer(n) => Value::BigInt(n.as_i64()),
                    rmpv::Value::Nil => Value::BigInt(None),
                    _ => Value::BigInt(None),
                })
                .collect(),
        ),
        "float" => (
            ArrayType::Double,
            arr.iter()
                .map(|v| match v {
                    rmpv::Value::F64(f) => Value::Double(Some(*f)),
                    rmpv::Value::F32(f) => Value::Double(Some(f64::from(*f))),
                    rmpv::Value::Integer(n) => {
                        Value::Double(n.as_f64().or_else(|| n.as_i64().map(|i| i as f64)))
                    }
                    rmpv::Value::Nil => Value::Double(None),
                    _ => Value::Double(None),
                })
                .collect(),
        ),
        "bool" => (
            ArrayType::Bool,
            arr.iter()
                .map(|v| match v {
                    rmpv::Value::Boolean(b) => Value::Bool(Some(*b)),
                    rmpv::Value::Nil => Value::Bool(None),
                    _ => Value::Bool(None),
                })
                .collect(),
        ),
        "uuid" => (
            ArrayType::Uuid,
            arr.iter()
                .map(|v| match v {
                    rmpv::Value::String(s) => {
                        let s = s.as_str().unwrap_or_default();
                        match uuid::Uuid::parse_str(s) {
                            Ok(u) => Value::Uuid(Some(Box::new(u))),
                            Err(_) => Value::Uuid(None),
                        }
                    }
                    rmpv::Value::Nil => Value::Uuid(None),
                    _ => Value::Uuid(None),
                })
                .collect(),
        ),
        "json" => (
            ArrayType::Json,
            arr.iter()
                .map(|v| match v {
                    rmpv::Value::Nil => Value::Json(None),
                    other => {
                        let json = rmpv_to_json(other);
                        Value::Json(Some(Box::new(json)))
                    }
                })
                .collect(),
        ),
        "datetime" => (
            ArrayType::ChronoDateTime,
            arr.iter()
                .map(|v| match v {
                    rmpv::Value::String(s) => {
                        let s = s.as_str().unwrap_or_default();
                        if let Ok(dt) = parse_datetime(s) {
                            Value::ChronoDateTime(Some(Box::new(dt)))
                        } else {
                            Value::ChronoDateTime(None)
                        }
                    }
                    rmpv::Value::Nil => Value::ChronoDateTime(None),
                    _ => Value::ChronoDateTime(None),
                })
                .collect(),
        ),
        // Fallback: treat as string array
        _ => (
            ArrayType::String,
            arr.iter()
                .map(|v| match v {
                    rmpv::Value::Nil => Value::String(None),
                    other => Value::String(Some(Box::new(format!("{other}")))),
                })
                .collect(),
        ),
    }
}

/// Convert an rmpv array to a serde_json::Value::Array
fn rmpv_array_to_json(arr: &[rmpv::Value]) -> serde_json::Value {
    serde_json::Value::Array(arr.iter().map(rmpv_to_json).collect())
}

/// Convert an rmpv value to serde_json::Value
fn rmpv_to_json(v: &rmpv::Value) -> serde_json::Value {
    match v {
        rmpv::Value::Nil => serde_json::Value::Null,
        rmpv::Value::Boolean(b) => serde_json::Value::Bool(*b),
        rmpv::Value::Integer(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                serde_json::Value::Number(u.into())
            } else {
                serde_json::Value::Null
            }
        }
        rmpv::Value::F32(f) => serde_json::Number::from_f64(f64::from(*f))
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        rmpv::Value::F64(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        rmpv::Value::String(s) => {
            serde_json::Value::String(s.as_str().unwrap_or_default().to_string())
        }
        rmpv::Value::Binary(b) => serde_json::Value::String(format!("{b:?}")),
        rmpv::Value::Array(arr) => rmpv_array_to_json(arr),
        rmpv::Value::Map(pairs) => {
            let obj: serde_json::Map<String, serde_json::Value> = pairs
                .iter()
                .filter_map(|(k, v)| k.as_str().map(|key| (key.to_string(), rmpv_to_json(v))))
                .collect();
            serde_json::Value::Object(obj)
        }
        rmpv::Value::Ext(_, _) => serde_json::Value::Null,
    }
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

    #[test]
    fn test_string_array_with_col_type() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::String("hello".into()),
            rmpv::Value::String("world".into()),
        ]);
        let val = rmpv_to_value_typed(&arr, Some("str[]"));
        match val {
            Value::Array(ArrayType::String, Some(elems)) => {
                assert_eq!(elems.len(), 2);
                assert!(matches!(&elems[0], Value::String(Some(s)) if s.as_ref() == "hello"));
                assert!(matches!(&elems[1], Value::String(Some(s)) if s.as_ref() == "world"));
            }
            other => panic!("expected Array(String, ...), got: {other:?}"),
        }
    }

    #[test]
    fn test_uuid_array_with_col_type() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::String("550e8400-e29b-41d4-a716-446655440000".into()),
            rmpv::Value::String("6ba7b810-9dad-11d1-80b4-00c04fd430c8".into()),
        ]);
        let val = rmpv_to_value_typed(&arr, Some("uuid[]"));
        match val {
            Value::Array(ArrayType::Uuid, Some(elems)) => {
                assert_eq!(elems.len(), 2);
                assert!(matches!(&elems[0], Value::Uuid(Some(_))));
                assert!(matches!(&elems[1], Value::Uuid(Some(_))));
            }
            other => panic!("expected Array(Uuid, ...), got: {other:?}"),
        }
    }

    #[test]
    fn test_int_array_with_col_type() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::Integer(1.into()),
            rmpv::Value::Integer(2.into()),
            rmpv::Value::Integer(3.into()),
        ]);
        let val = rmpv_to_value_typed(&arr, Some("int[]"));
        match val {
            Value::Array(ArrayType::BigInt, Some(elems)) => {
                assert_eq!(elems.len(), 3);
                assert!(matches!(&elems[0], Value::BigInt(Some(1))));
                assert!(matches!(&elems[1], Value::BigInt(Some(2))));
                assert!(matches!(&elems[2], Value::BigInt(Some(3))));
            }
            other => panic!("expected Array(BigInt, ...), got: {other:?}"),
        }
    }

    #[test]
    fn test_float_array_with_col_type() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::F64(1.5),
            rmpv::Value::F64(2.7),
        ]);
        let val = rmpv_to_value_typed(&arr, Some("float[]"));
        match val {
            Value::Array(ArrayType::Double, Some(elems)) => {
                assert_eq!(elems.len(), 2);
                assert!(matches!(&elems[0], Value::Double(Some(v)) if (*v - 1.5).abs() < f64::EPSILON));
                assert!(matches!(&elems[1], Value::Double(Some(v)) if (*v - 2.7).abs() < f64::EPSILON));
            }
            other => panic!("expected Array(Double, ...), got: {other:?}"),
        }
    }

    #[test]
    fn test_bool_array_with_col_type() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::Boolean(true),
            rmpv::Value::Boolean(false),
        ]);
        let val = rmpv_to_value_typed(&arr, Some("bool[]"));
        match val {
            Value::Array(ArrayType::Bool, Some(elems)) => {
                assert_eq!(elems.len(), 2);
                assert!(matches!(&elems[0], Value::Bool(Some(true))));
                assert!(matches!(&elems[1], Value::Bool(Some(false))));
            }
            other => panic!("expected Array(Bool, ...), got: {other:?}"),
        }
    }

    #[test]
    fn test_empty_array_with_col_type() {
        let arr = rmpv::Value::Array(vec![]);
        let val = rmpv_to_value_typed(&arr, Some("str[]"));
        match val {
            Value::Array(ArrayType::String, Some(elems)) => {
                assert!(elems.is_empty());
            }
            other => panic!("expected empty Array(String, ...), got: {other:?}"),
        }
    }

    #[test]
    fn test_array_with_nil_elements() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::String("hello".into()),
            rmpv::Value::Nil,
            rmpv::Value::String("world".into()),
        ]);
        let val = rmpv_to_value_typed(&arr, Some("str[]"));
        match val {
            Value::Array(ArrayType::String, Some(elems)) => {
                assert_eq!(elems.len(), 3);
                assert!(matches!(&elems[0], Value::String(Some(s)) if s.as_ref() == "hello"));
                assert!(matches!(&elems[1], Value::String(None)));
                assert!(matches!(&elems[2], Value::String(Some(s)) if s.as_ref() == "world"));
            }
            other => panic!("expected Array with nil element, got: {other:?}"),
        }
    }

    #[test]
    fn test_array_without_col_type_falls_back_to_string() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::String("a".into()),
            rmpv::Value::String("b".into()),
        ]);
        let val = rmpv_to_value_typed(&arr, None);
        // Without col_type, falls back to string serialization
        assert!(matches!(val, Value::String(Some(_))));
    }

    #[test]
    fn test_array_with_json_col_type() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::Integer(1.into()),
            rmpv::Value::String("two".into()),
        ]);
        let val = rmpv_to_value_typed(&arr, Some("json"));
        assert!(matches!(val, Value::Json(Some(_))));
    }

    #[test]
    fn test_array_with_jsonb_col_type() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::Boolean(true),
            rmpv::Value::Nil,
        ]);
        let val = rmpv_to_value_typed(&arr, Some("jsonb"));
        assert!(matches!(val, Value::Json(Some(_))));
    }

    #[test]
    fn test_datetime_array_with_col_type() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::String("2024-01-15 10:30:00".into()),
            rmpv::Value::String("2024-06-20T14:00:00".into()),
        ]);
        let val = rmpv_to_value_typed(&arr, Some("datetime[]"));
        match val {
            Value::Array(ArrayType::ChronoDateTime, Some(elems)) => {
                assert_eq!(elems.len(), 2);
                assert!(matches!(&elems[0], Value::ChronoDateTime(Some(_))));
                assert!(matches!(&elems[1], Value::ChronoDateTime(Some(_))));
            }
            other => panic!("expected Array(ChronoDateTime, ...), got: {other:?}"),
        }
    }

    #[test]
    fn test_json_array_elements_with_json_array_col_type() {
        let arr = rmpv::Value::Array(vec![
            rmpv::Value::Map(vec![
                (rmpv::Value::String("key".into()), rmpv::Value::String("val".into())),
            ]),
        ]);
        let val = rmpv_to_value_typed(&arr, Some("json[]"));
        match val {
            Value::Array(ArrayType::Json, Some(elems)) => {
                assert_eq!(elems.len(), 1);
                assert!(matches!(&elems[0], Value::Json(Some(_))));
            }
            other => panic!("expected Array(Json, ...), got: {other:?}"),
        }
    }

    // ── Scalar conversion tests (ensure no regressions) ──

    #[test]
    fn test_scalar_uuid_with_col_type() {
        let v = rmpv::Value::String("550e8400-e29b-41d4-a716-446655440000".into());
        let val = rmpv_to_value_typed(&v, Some("uuid"));
        assert!(matches!(val, Value::Uuid(Some(_))));
    }

    #[test]
    fn test_scalar_uuid_invalid_falls_back() {
        let v = rmpv::Value::String("not-a-uuid".into());
        let val = rmpv_to_value_typed(&v, Some("uuid"));
        // Invalid UUID stays as string
        assert!(matches!(val, Value::String(Some(_))));
    }

    #[test]
    fn test_scalar_jsonb_string_with_col_type() {
        let v = rmpv::Value::String(r#"{"key": "value"}"#.into());
        let val = rmpv_to_value_typed(&v, Some("jsonb"));
        match val {
            Value::Json(Some(j)) => {
                assert_eq!(j.as_ref()["key"], "value");
            }
            other => panic!("expected Json, got: {other:?}"),
        }
    }

    #[test]
    fn test_scalar_json_string_with_col_type() {
        let v = rmpv::Value::String(r#"[1, 2, 3]"#.into());
        let val = rmpv_to_value_typed(&v, Some("json"));
        assert!(matches!(val, Value::Json(Some(_))));
    }

    #[test]
    fn test_scalar_jsonb_plain_string_wraps_as_json_string() {
        let v = rmpv::Value::String("hello".into());
        let val = rmpv_to_value_typed(&v, Some("jsonb"));
        match val {
            Value::Json(Some(j)) => {
                assert_eq!(*j, serde_json::Value::String("hello".into()));
            }
            other => panic!("expected Json string, got: {other:?}"),
        }
    }

    #[test]
    fn test_map_becomes_json_with_jsonb_col_type() {
        let v = rmpv::Value::Map(vec![(
            rmpv::Value::String("key".into()),
            rmpv::Value::String("val".into()),
        )]);
        let val = rmpv_to_value_typed(&v, Some("jsonb"));
        match val {
            Value::Json(Some(j)) => {
                assert_eq!(j.as_ref()["key"], "val");
            }
            other => panic!("expected Json, got: {other:?}"),
        }
    }

    #[test]
    fn test_map_becomes_json_without_col_type() {
        let v = rmpv::Value::Map(vec![(
            rmpv::Value::String("a".into()),
            rmpv::Value::Integer(1.into()),
        )]);
        let val = rmpv_to_value_typed(&v, None);
        assert!(matches!(val, Value::Json(Some(_))));
    }

    #[test]
    fn test_scalar_string_unchanged() {
        let v = rmpv::Value::String("hello".into());
        let val = rmpv_to_value_typed(&v, Some("str"));
        assert!(matches!(val, Value::String(Some(s)) if s.as_ref() == "hello"));
    }

    #[test]
    fn test_scalar_int_unchanged() {
        let v = rmpv::Value::Integer(42.into());
        let val = rmpv_to_value_typed(&v, None);
        assert!(matches!(val, Value::BigInt(Some(42))));
    }

    #[test]
    fn test_scalar_nil_unchanged() {
        let v = rmpv::Value::Nil;
        let val = rmpv_to_value_typed(&v, None);
        assert!(matches!(val, Value::String(None)));
    }
}

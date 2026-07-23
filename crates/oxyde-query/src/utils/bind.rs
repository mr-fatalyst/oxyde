//! `ColumnTypeSpec`-driven value binding: `rmpv::Value` + spec → `sea_query::Value`.
//!
//! The single binding path. The test matrix below is the behavioral spec
//! (typed nulls, datetime heuristics, fallbacks) — any change to it is a
//! conscious, documented decision.
//!
//! Dialect note: binding itself is dialect-independent; per-dialect value
//! adaptation (e.g. arrays → JSON on MySQL/SQLite) happens in the driver's
//! bind layer.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use oxyde_codec::ColumnTypeSpec;
use rust_decimal::Decimal;
use sea_query::value::ArrayType;
use sea_query::{Expr, SimpleExpr, Value};
use uuid::Uuid;

use crate::Dialect;

/// Convert an rmpv value without a column spec (raw SQL parameters).
/// Identical to `bind_value(value, &ColumnTypeSpec::Unknown)`.
pub fn rmpv_to_value(value: &rmpv::Value) -> Value {
    bind_value(value, &ColumnTypeSpec::Unknown)
}

/// Convert an rmpv value to a sea_query Value using the column type spec.
pub fn bind_value(value: &rmpv::Value, spec: &ColumnTypeSpec) -> Value {
    match value {
        rmpv::Value::Nil => typed_null(spec),
        rmpv::Value::Boolean(b) => Value::Bool(Some(*b)),
        rmpv::Value::Integer(n) => {
            if let Some(i) = n.as_i64() {
                Value::BigInt(Some(i))
            } else if let Some(u) = n.as_u64() {
                // u64 that doesn't fit i64 — store as string (legacy behavior)
                Value::String(Some(Box::new(u.to_string())))
            } else {
                Value::String(Some(Box::new(n.to_string())))
            }
        }
        rmpv::Value::F32(f) => Value::Double(Some(f64::from(*f))),
        rmpv::Value::F64(f) => Value::Double(Some(*f)),
        rmpv::Value::String(s) => bind_string(s.as_str().unwrap_or_default(), spec),
        rmpv::Value::Binary(b) => Value::Bytes(Some(Box::new(b.clone()))),
        rmpv::Value::Map(_) => {
            if matches!(spec, ColumnTypeSpec::Json | ColumnTypeSpec::JsonBinary) {
                if let Some(j) = super::value::rmpv_to_json(value) {
                    return Value::Json(Some(Box::new(j)));
                }
            }
            Value::String(Some(Box::new(format!("{value}"))))
        }
        rmpv::Value::Array(arr) => match spec {
            ColumnTypeSpec::Json | ColumnTypeSpec::JsonBinary => {
                match super::value::rmpv_to_json(value) {
                    Some(j) => Value::Json(Some(Box::new(j))),
                    None => Value::String(Some(Box::new(format!("{value}")))),
                }
            }
            ColumnTypeSpec::Array { item } => match element_array_type(item) {
                Some(arr_type) => {
                    let values: Vec<Value> = arr.iter().map(|v| bind_value(v, item)).collect();
                    Value::Array(arr_type, Some(Box::new(values)))
                }
                // Unknown / nested-array element types have no sea_query
                // ArrayType — stringify, matching the legacy unclassified path.
                None => Value::String(Some(Box::new(format!("{value}")))),
            },
            _ => Value::String(Some(Box::new(format!("{value}")))),
        },
        rmpv::Value::Ext(_, _) => Value::String(Some(Box::new(format!("{value}")))),
    }
}

pub fn typed_value_expr(value: Value, spec: &ColumnTypeSpec, dialect: Dialect) -> SimpleExpr {
    if dialect == Dialect::Postgres {
        if let Some(type_name) = postgres_enum_cast_type(spec) {
            return Expr::cust_with_values(format!("$1::{type_name}"), vec![value]);
        }
    }
    Expr::val(value).into()
}

/// Bind a string payload according to the spec.
///
/// Temporal specs and `Unknown` fall back to the RFC3339 heuristic on parse
/// failure (legacy behavior); all other specs return the string as-is —
/// a `str`/`uuid`/`decimal` hint must never silently become a datetime.
fn bind_string(s: &str, spec: &ColumnTypeSpec) -> Value {
    match spec {
        ColumnTypeSpec::DateTimeUtc => {
            if let Some(dt) = parse_datetime_utc(s) {
                return Value::ChronoDateTimeUtc(Some(Box::new(dt)));
            }
            // Naive string under an aware spec: bind as naive (legacy fallback)
            if let Ok(dt) = parse_datetime(s) {
                return Value::ChronoDateTime(Some(Box::new(dt)));
            }
            string_value(s)
        }
        ColumnTypeSpec::DateTime => {
            if let Ok(dt) = parse_datetime(s) {
                return Value::ChronoDateTime(Some(Box::new(dt)));
            }
            // Aware string under a naive spec: normalize to UTC (legacy fallback)
            rfc3339_heuristic_or_string(s)
        }
        ColumnTypeSpec::Date => {
            if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                return Value::ChronoDate(Some(Box::new(d)));
            }
            rfc3339_heuristic_or_string(s)
        }
        ColumnTypeSpec::Time => {
            if let Ok(t) = parse_time(s) {
                return Value::ChronoTime(Some(Box::new(t)));
            }
            rfc3339_heuristic_or_string(s)
        }
        ColumnTypeSpec::Uuid => match Uuid::parse_str(s) {
            Ok(u) => Value::Uuid(Some(Box::new(u))),
            Err(_) => string_value(s),
        },
        ColumnTypeSpec::Decimal { .. } => match s.parse::<Decimal>() {
            Ok(d) => Value::Decimal(Some(Box::new(d))),
            Err(_) => string_value(s),
        },
        ColumnTypeSpec::Json | ColumnTypeSpec::JsonBinary => {
            match serde_json::from_str::<serde_json::Value>(s) {
                Ok(j) => Value::Json(Some(Box::new(j))),
                Err(_) => string_value(s),
            }
        }
        ColumnTypeSpec::Enum { .. } => string_value(s),
        // Known non-temporal specs: definitely not a datetime — no heuristic.
        ColumnTypeSpec::BigInteger
        | ColumnTypeSpec::Double
        | ColumnTypeSpec::Boolean
        | ColumnTypeSpec::Text
        | ColumnTypeSpec::String { .. }
        | ColumnTypeSpec::Blob
        | ColumnTypeSpec::Timedelta => string_value(s),
        // No binding knowledge: legacy no-hint behavior incl. the heuristic.
        ColumnTypeSpec::Array { .. } | ColumnTypeSpec::Unknown => rfc3339_heuristic_or_string(s),
    }
}

/// Produce a typed NULL for the spec.
fn typed_null(spec: &ColumnTypeSpec) -> Value {
    match spec {
        ColumnTypeSpec::BigInteger | ColumnTypeSpec::Timedelta => Value::BigInt(None),
        ColumnTypeSpec::Double => Value::Double(None),
        ColumnTypeSpec::Boolean => Value::Bool(None),
        ColumnTypeSpec::Text | ColumnTypeSpec::String { .. } | ColumnTypeSpec::Enum { .. } => {
            Value::String(None)
        }
        ColumnTypeSpec::Blob => Value::Bytes(None),
        ColumnTypeSpec::DateTime => Value::ChronoDateTime(None),
        ColumnTypeSpec::DateTimeUtc => Value::ChronoDateTimeUtc(None),
        ColumnTypeSpec::Date => Value::ChronoDate(None),
        ColumnTypeSpec::Time => Value::ChronoTime(None),
        ColumnTypeSpec::Uuid => Value::Uuid(None),
        ColumnTypeSpec::Decimal { .. } => Value::Decimal(None),
        ColumnTypeSpec::Json | ColumnTypeSpec::JsonBinary => Value::Json(None),
        ColumnTypeSpec::Array { item } => match element_array_type(item) {
            Some(arr_type) => Value::Array(arr_type, None),
            None => Value::String(None),
        },
        ColumnTypeSpec::Unknown => Value::String(None),
    }
}

/// sea_query element type for an array item spec.
/// `None` for items that sea_query arrays cannot carry (Unknown, nested arrays).
fn element_array_type(item: &ColumnTypeSpec) -> Option<ArrayType> {
    match item {
        ColumnTypeSpec::BigInteger | ColumnTypeSpec::Timedelta => Some(ArrayType::BigInt),
        ColumnTypeSpec::Double => Some(ArrayType::Double),
        ColumnTypeSpec::Boolean => Some(ArrayType::Bool),
        ColumnTypeSpec::Text | ColumnTypeSpec::String { .. } | ColumnTypeSpec::Enum { .. } => {
            Some(ArrayType::String)
        }
        ColumnTypeSpec::Blob => Some(ArrayType::Bytes),
        ColumnTypeSpec::DateTime => Some(ArrayType::ChronoDateTime),
        ColumnTypeSpec::DateTimeUtc => Some(ArrayType::ChronoDateTimeUtc),
        ColumnTypeSpec::Date => Some(ArrayType::ChronoDate),
        ColumnTypeSpec::Time => Some(ArrayType::ChronoTime),
        ColumnTypeSpec::Uuid => Some(ArrayType::Uuid),
        ColumnTypeSpec::Decimal { .. } => Some(ArrayType::Decimal),
        ColumnTypeSpec::Json | ColumnTypeSpec::JsonBinary => Some(ArrayType::Json),
        ColumnTypeSpec::Array { .. } | ColumnTypeSpec::Unknown => None,
    }
}

fn postgres_enum_cast_type(spec: &ColumnTypeSpec) -> Option<String> {
    match spec {
        ColumnTypeSpec::Enum { name, .. } => Some(quote_pg_type_path(name)),
        ColumnTypeSpec::Array { item } => match item.as_ref() {
            ColumnTypeSpec::Enum { name, .. } => Some(format!("{}[]", quote_pg_type_path(name))),
            _ => None,
        },
        _ => None,
    }
}

/// Keep in sync with `quote_postgres_type_name` in oxyde-migrate (parity tests).
fn quote_pg_type_path(name: &str) -> String {
    name.split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn string_value(s: &str) -> Value {
    Value::String(Some(Box::new(s.to_string())))
}

fn rfc3339_heuristic_or_string(s: &str) -> Value {
    match parse_datetime_utc(s) {
        Some(dt) => Value::ChronoDateTimeUtc(Some(Box::new(dt))),
        None => string_value(s),
    }
}

fn parse_datetime_utc(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

fn parse_datetime(s: &str) -> Result<NaiveDateTime, chrono::ParseError> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f"))
}

fn parse_time(s: &str) -> Result<NaiveTime, chrono::ParseError> {
    NaiveTime::parse_from_str(s, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S%.f"))
}

// ─────────────────────────────────────────────────────────────────────────
// Ported behavioral matrix from value.rs (string hints → specs).
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const UUID_STR: &str = "550e8400-e29b-41d4-a716-446655440000";

    fn s(v: &str) -> rmpv::Value {
        rmpv::Value::String(v.into())
    }

    fn string_spec() -> ColumnTypeSpec {
        ColumnTypeSpec::String { length: None }
    }

    #[test]
    fn test_quote_pg_type_path_parity_vectors() {
        assert_eq!(quote_pg_type_path("status_enum"), r#""status_enum""#);
        assert_eq!(
            quote_pg_type_path("public.status_enum"),
            r#""public"."status_enum""#
        );
        assert_eq!(quote_pg_type_path(r#"we"ird"#), r#""we""ird""#);
    }

    fn decimal_spec() -> ColumnTypeSpec {
        ColumnTypeSpec::Decimal {
            precision: None,
            scale: None,
        }
    }

    fn array_of(item: ColumnTypeSpec) -> ColumnTypeSpec {
        ColumnTypeSpec::Array {
            item: Box::new(item),
        }
    }

    // ── strings into typed specs ──────────────────────────────────────

    #[test]
    fn uuid_string() {
        match bind_value(&s(UUID_STR), &ColumnTypeSpec::Uuid) {
            Value::Uuid(Some(u)) => assert_eq!(u.to_string(), UUID_STR),
            other => panic!("expected Uuid, got {other:?}"),
        }
    }

    #[test]
    fn uuid_invalid_falls_back_to_string() {
        assert!(matches!(
            bind_value(&s("not-a-uuid"), &ColumnTypeSpec::Uuid),
            Value::String(Some(_))
        ));
    }

    #[test]
    fn decimal_string() {
        match bind_value(&s("99.99"), &decimal_spec()) {
            Value::Decimal(Some(d)) => assert_eq!(*d, Decimal::from_str("99.99").unwrap()),
            other => panic!("expected Decimal, got {other:?}"),
        }
    }

    #[test]
    fn decimal_with_precision_parses_same() {
        let spec = ColumnTypeSpec::Decimal {
            precision: Some(10),
            scale: Some(3),
        };
        assert!(matches!(
            bind_value(&s("123.456"), &spec),
            Value::Decimal(Some(_))
        ));
    }

    #[test]
    fn json_string() {
        match bind_value(&s(r#"{"key": "value"}"#), &ColumnTypeSpec::Json) {
            Value::Json(Some(j)) => assert_eq!(j["key"], "value"),
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn json_map_value() {
        let val = rmpv::Value::Map(vec![(
            rmpv::Value::String("key".into()),
            rmpv::Value::String("value".into()),
        )]);
        match bind_value(&val, &ColumnTypeSpec::JsonBinary) {
            Value::Json(Some(j)) => assert_eq!(j["key"], "value"),
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn json_array_value() {
        let val = rmpv::Value::Array(vec![
            rmpv::Value::Integer(1.into()),
            rmpv::Value::Integer(2.into()),
        ]);
        match bind_value(&val, &ColumnTypeSpec::Json) {
            Value::Json(Some(j)) => {
                assert_eq!(j.as_array().unwrap().len(), 2);
                assert_eq!(j[0], 1);
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn map_without_json_spec_stringifies() {
        let val = rmpv::Value::Map(vec![(
            rmpv::Value::String("key".into()),
            rmpv::Value::String("value".into()),
        )]);
        assert!(matches!(
            bind_value(&val, &ColumnTypeSpec::Unknown),
            Value::String(Some(_))
        ));
    }

    // ── typed nulls ───────────────────────────────────────────────────

    #[test]
    fn typed_nulls() {
        let nil = rmpv::Value::Nil;
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::BigInteger),
            Value::BigInt(None)
        ));
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::Uuid),
            Value::Uuid(None)
        ));
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::JsonBinary),
            Value::Json(None)
        ));
        assert!(matches!(
            bind_value(&nil, &decimal_spec()),
            Value::Decimal(None)
        ));
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::DateTime),
            Value::ChronoDateTime(None)
        ));
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::DateTimeUtc),
            Value::ChronoDateTimeUtc(None)
        ));
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::Boolean),
            Value::Bool(None)
        ));
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::Double),
            Value::Double(None)
        ));
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::Blob),
            Value::Bytes(None)
        ));
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::Timedelta),
            Value::BigInt(None)
        ));
        assert!(matches!(
            bind_value(&nil, &ColumnTypeSpec::Unknown),
            Value::String(None)
        ));
        assert!(matches!(
            bind_value(&nil, &array_of(ColumnTypeSpec::BigInteger)),
            Value::Array(ArrayType::BigInt, None)
        ));
    }

    // ── datetime semantics (the heuristic matrix) ─────────────────────

    #[test]
    fn datetime_naive_string() {
        assert!(matches!(
            bind_value(&s("2024-01-15T10:30:00"), &ColumnTypeSpec::DateTime),
            Value::ChronoDateTime(Some(_))
        ));
    }

    #[test]
    fn timestamptz_aware_string() {
        assert!(matches!(
            bind_value(
                &s("2024-01-15T10:30:00+00:00"),
                &ColumnTypeSpec::DateTimeUtc
            ),
            Value::ChronoDateTimeUtc(Some(_))
        ));
    }

    #[test]
    fn timestamptz_naive_string_falls_back_to_naive() {
        assert!(matches!(
            bind_value(&s("2024-01-15T10:30:00"), &ColumnTypeSpec::DateTimeUtc),
            Value::ChronoDateTime(Some(_))
        ));
    }

    #[test]
    fn datetime_spec_with_tz_string_normalizes_to_utc() {
        assert!(matches!(
            bind_value(&s("2024-01-15T12:30:00+03:00"), &ColumnTypeSpec::DateTime),
            Value::ChronoDateTimeUtc(Some(_))
        ));
    }

    #[test]
    fn string_specs_never_parse_datetime() {
        for spec in [
            ColumnTypeSpec::Text,
            string_spec(),
            ColumnTypeSpec::String { length: Some(100) },
        ] {
            match bind_value(&s("2024-01-15T12:30:00Z"), &spec) {
                Value::String(Some(v)) => assert_eq!(v.as_ref(), "2024-01-15T12:30:00Z"),
                other => panic!("spec {spec:?}: expected String, got {other:?}"),
            }
        }
    }

    #[test]
    fn uuid_spec_with_datetime_string_stays_string() {
        assert!(matches!(
            bind_value(&s("2024-01-15T12:30:00Z"), &ColumnTypeSpec::Uuid),
            Value::String(Some(_))
        ));
    }

    #[test]
    fn unknown_spec_keeps_rfc3339_heuristic() {
        assert!(matches!(
            bind_value(&s("2024-01-15T12:30:00Z"), &ColumnTypeSpec::Unknown),
            Value::ChronoDateTimeUtc(Some(_))
        ));
    }

    /// Legacy quirk, locked consciously: a full RFC3339 string under a Date
    /// spec falls through the date parser into the datetime heuristic.
    #[test]
    fn date_spec_with_datetime_string_becomes_datetime() {
        assert!(matches!(
            bind_value(&s("2024-01-15T10:00:00Z"), &ColumnTypeSpec::Date),
            Value::ChronoDateTimeUtc(Some(_))
        ));
    }

    // ── arrays ────────────────────────────────────────────────────────

    #[test]
    fn array_int() {
        let val = rmpv::Value::Array(vec![
            rmpv::Value::Integer(1.into()),
            rmpv::Value::Integer(2.into()),
            rmpv::Value::Integer(3.into()),
        ]);
        match bind_value(&val, &array_of(ColumnTypeSpec::BigInteger)) {
            Value::Array(ArrayType::BigInt, Some(vals)) => {
                assert_eq!(vals.len(), 3);
                assert!(matches!(vals[0], Value::BigInt(Some(1))));
                assert!(matches!(vals[2], Value::BigInt(Some(3))));
            }
            other => panic!("expected Array(BigInt), got {other:?}"),
        }
    }

    #[test]
    fn array_str() {
        let val = rmpv::Value::Array(vec![
            rmpv::Value::String("a".into()),
            rmpv::Value::String("b".into()),
        ]);
        match bind_value(&val, &array_of(ColumnTypeSpec::Text)) {
            Value::Array(ArrayType::String, Some(vals)) => assert_eq!(vals.len(), 2),
            other => panic!("expected Array(String), got {other:?}"),
        }
    }

    #[test]
    fn array_uuid() {
        let val = rmpv::Value::Array(vec![rmpv::Value::String(UUID_STR.into())]);
        match bind_value(&val, &array_of(ColumnTypeSpec::Uuid)) {
            Value::Array(ArrayType::Uuid, Some(vals)) => {
                assert_eq!(vals.len(), 1);
                assert!(matches!(vals[0], Value::Uuid(Some(_))));
            }
            other => panic!("expected Array(Uuid), got {other:?}"),
        }
    }

    #[test]
    fn array_with_null_element() {
        let val = rmpv::Value::Array(vec![
            rmpv::Value::Integer(1.into()),
            rmpv::Value::Nil,
            rmpv::Value::Integer(3.into()),
        ]);
        match bind_value(&val, &array_of(ColumnTypeSpec::BigInteger)) {
            Value::Array(ArrayType::BigInt, Some(vals)) => {
                assert!(matches!(vals[1], Value::BigInt(None)));
            }
            other => panic!("expected Array(BigInt), got {other:?}"),
        }
    }

    #[test]
    fn array_without_array_spec_stringifies() {
        let val = rmpv::Value::Array(vec![rmpv::Value::Integer(1.into())]);
        assert!(matches!(
            bind_value(&val, &ColumnTypeSpec::Unknown),
            Value::String(Some(_))
        ));
    }

    #[test]
    fn array_of_unknown_items_stringifies() {
        let val = rmpv::Value::Array(vec![rmpv::Value::Integer(1.into())]);
        assert!(matches!(
            bind_value(&val, &array_of(ColumnTypeSpec::Unknown)),
            Value::String(Some(_))
        ));
    }

    // ── native conversions (spec-independent) ─────────────────────────

    #[test]
    fn native_scalars() {
        assert!(matches!(
            bind_value(&rmpv::Value::Integer(42.into()), &ColumnTypeSpec::Unknown),
            Value::BigInt(Some(42))
        ));
        assert!(matches!(
            bind_value(&rmpv::Value::Boolean(true), &ColumnTypeSpec::Unknown),
            Value::Bool(Some(true))
        ));
        assert!(matches!(
            bind_value(&rmpv::Value::F64(1.5), &ColumnTypeSpec::Unknown),
            Value::Double(Some(_))
        ));
        match bind_value(&s("hello"), &ColumnTypeSpec::Unknown) {
            Value::String(Some(v)) => assert_eq!(v.as_ref(), "hello"),
            other => panic!("expected String, got {other:?}"),
        }
    }
}

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
#[must_use]
pub fn rmpv_to_value(value: &rmpv::Value) -> Value {
    bind_value(value, &ColumnTypeSpec::Unknown)
}

/// Convert an rmpv value to a sea_query Value using the column type spec.
#[must_use]
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
                    let values: Vec<Value> = arr.iter().map(|v| bind_element(v, item)).collect();
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

/// Bind one array element as the value kind its item spec stores.
/// PostgreSQL array binding drops elements of any other kind as NULL, and
/// the JSON encoder (MySQL/SQLite) writes them in another format.
///
/// Elements of a `Json` / `JsonBinary` item keep their own JSON type: a
/// string is a JSON string, never parsed as JSON text, so `"123"` in a
/// `list[str]` stays a string. (A whole JSON column holds the document a
/// `str` field gives, so `bind_string` parses that.)
fn bind_element(value: &rmpv::Value, item: &ColumnTypeSpec) -> Value {
    match item {
        ColumnTypeSpec::Json | ColumnTypeSpec::JsonBinary if !value.is_nil() => {
            match super::value::rmpv_to_json(value) {
                Some(json) => Value::Json(Some(Box::new(json))),
                None => typed_null(item),
            }
        }
        _ => coerce_element(bind_value(value, item), item),
    }
}

/// Give a bound array element the one value kind its item spec stores;
/// other values pass through unchanged.
///
/// - An aware/naive datetime takes the flavour of a `DateTime` /
///   `DateTimeUtc` spec (via UTC).
/// - An integer becomes a float or decimal in a `Double` / `Decimal` spec.
fn coerce_element(value: Value, spec: &ColumnTypeSpec) -> Value {
    match (spec, value) {
        (ColumnTypeSpec::DateTime, Value::ChronoDateTimeUtc(Some(dt))) => {
            Value::ChronoDateTime(Some(Box::new(dt.naive_utc())))
        }
        (ColumnTypeSpec::DateTimeUtc, Value::ChronoDateTime(Some(dt))) => {
            Value::ChronoDateTimeUtc(Some(Box::new(dt.and_utc())))
        }
        // The column holds doubles, so the comparison happens in double
        // precision whatever the integer.
        #[allow(clippy::cast_precision_loss)]
        (ColumnTypeSpec::Double, Value::BigInt(Some(n))) => Value::Double(Some(n as f64)),
        (ColumnTypeSpec::Decimal { .. }, Value::BigInt(Some(n))) => {
            Value::Decimal(Some(Box::new(Decimal::from(n))))
        }
        (_, value) => value,
    }
}

/// Parse a decimal string; Python's `str(Decimal)` may use an exponent.
fn parse_decimal(s: &str) -> Option<Decimal> {
    s.parse::<Decimal>()
        .or_else(|_| Decimal::from_scientific(s))
        .ok()
        // Exact values the two parsers miss: zeros with a large exponent
        // ("0E-30") and mantissas with long runs of trailing zeros.
        .or_else(|| parse_decimal_exact(s))
}

/// `parse_decimal` without rounding: `None` for a value a `Decimal` cannot
/// hold exactly (more than 28 fractional digits once trailing zeros go).
fn parse_decimal_exact(s: &str) -> Option<Decimal> {
    // Keep the scale of ordinary decimal strings when they already fit.
    if let Ok(decimal) = Decimal::from_str_exact(s) {
        return Some(decimal);
    }
    let (mantissa, exponent) = match s.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i64>().ok()?),
        None => (s, 0),
    };
    let (negative, unsigned) = match mantissa.as_bytes().first()? {
        b'-' => (true, &mantissa[1..]),
        b'+' => (false, &mantissa[1..]),
        _ => (false, mantissa),
    };

    // Read the coefficient as text: it may exceed Decimal's limits before
    // the exponent is applied (1e30 * 1e-2 is the exactly representable 1e28).
    let mut coefficient = String::with_capacity(unsigned.len());
    let mut fractional_digits = 0i64;
    let mut point = false;
    for byte in unsigned.bytes() {
        match byte {
            b'0'..=b'9' => {
                coefficient.push(char::from(byte));
                if point {
                    fractional_digits = fractional_digits.checked_add(1)?;
                }
            }
            b'.' if !point => point = true,
            // Decimal accepts separators after the first digit.
            b'_' if !coefficient.is_empty() => {}
            _ => return None,
        }
    }
    if coefficient.is_empty() {
        return None;
    }
    let significant = coefficient.trim_start_matches('0');
    // Zero is exact whatever its exponent, without any scale arithmetic.
    if significant.is_empty() {
        return Some(Decimal::ZERO);
    }

    // Remove only zeros, never significant digits, before imposing either
    // the 96-bit coefficient limit or the 28-place scale limit.
    let digits = significant.trim_end_matches('0');
    let trailing_zeros = i64::try_from(significant.len() - digits.len()).ok()?;
    let mut scale = fractional_digits
        .checked_sub(exponent)?
        .checked_sub(trailing_zeros)?;
    let mut digits = digits.parse::<i128>().ok()?;
    if negative {
        digits = -digits;
    }
    if scale < 0 {
        let power = 10i128.checked_pow(u32::try_from(scale.checked_neg()?).ok()?)?;
        digits = digits.checked_mul(power)?;
        scale = 0;
    }
    let scale = u32::try_from(scale)
        .ok()
        .filter(|&s| s <= Decimal::MAX_SCALE)?;
    Decimal::try_from_i128_with_scale(digits, scale).ok()
}

#[must_use]
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
        ColumnTypeSpec::Decimal { .. } => match parse_decimal(s) {
            Some(d) => Value::Decimal(Some(Box::new(d))),
            None => string_value(s),
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

/// The first element of an array value that is not of the array's element
/// type, such as a float in a `Decimal` array or an integer beyond 64 bits
/// (bound as a string) in an integer one. NULL elements bind as their
/// item's typed NULL, so they always fit.
///
/// Only PostgreSQL arrays are typed: their binding drops such an element as
/// NULL, and NULL elements compare equal there, so a filter value with one
/// would match the wrong rows. JSON arrays encode each element as given.
pub(crate) fn mismatched_array_element(value: &Value) -> Option<&Value> {
    let Value::Array(arr_type, Some(elements)) = value else {
        return None;
    };
    elements.iter().find(|element| {
        !matches!(
            (arr_type, element),
            (ArrayType::Bool, Value::Bool(_))
                | (ArrayType::BigInt, Value::BigInt(_))
                | (ArrayType::Double, Value::Double(_))
                | (ArrayType::String, Value::String(_))
                | (ArrayType::Bytes, Value::Bytes(_))
                | (ArrayType::ChronoDateTime, Value::ChronoDateTime(_))
                | (ArrayType::ChronoDateTimeUtc, Value::ChronoDateTimeUtc(_))
                | (ArrayType::ChronoDate, Value::ChronoDate(_))
                | (ArrayType::ChronoTime, Value::ChronoTime(_))
                | (ArrayType::Uuid, Value::Uuid(_))
                | (ArrayType::Decimal, Value::Decimal(_))
                | (ArrayType::Json, Value::Json(_))
        )
    })
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

/// Quote a (possibly schema-qualified) PG type name: `a.b` → `"a"."b"`.
pub(crate) fn quote_pg_type_path(name: &str) -> String {
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
    fn test_quote_pg_type_path() {
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
    fn datetime_array_elements_take_the_spec_flavour() {
        let val = rmpv::Value::Array(vec![
            s("2024-01-01T10:00:00+02:00"),
            s("2024-01-01T10:00:00"),
        ]);

        match bind_value(&val, &array_of(ColumnTypeSpec::DateTime)) {
            Value::Array(ArrayType::ChronoDateTime, Some(vals)) => {
                let expected = NaiveDateTime::from_str("2024-01-01T08:00:00").unwrap();
                assert_eq!(vals[0], Value::ChronoDateTime(Some(Box::new(expected))));
                assert!(matches!(vals[1], Value::ChronoDateTime(Some(_))));
            }
            other => panic!("expected Array(ChronoDateTime), got {other:?}"),
        }

        match bind_value(&val, &array_of(ColumnTypeSpec::DateTimeUtc)) {
            Value::Array(ArrayType::ChronoDateTimeUtc, Some(vals)) => {
                let expected = NaiveDateTime::from_str("2024-01-01T10:00:00")
                    .unwrap()
                    .and_utc();
                assert!(matches!(vals[0], Value::ChronoDateTimeUtc(Some(_))));
                assert_eq!(vals[1], Value::ChronoDateTimeUtc(Some(Box::new(expected))));
            }
            other => panic!("expected Array(ChronoDateTimeUtc), got {other:?}"),
        }
    }

    #[test]
    fn json_array_elements_keep_their_json_type() {
        let val = rmpv::Value::Array(vec![
            rmpv::Value::Map(vec![(s("a"), 1.into())]),
            5.into(),
            true.into(),
            rmpv::Value::F64(1.5),
            // Strings are JSON strings, even when they read as JSON text.
            s("hello"),
            s("123"),
            s("null"),
            s(r#"{"a": 1}"#),
            rmpv::Value::Nil,
        ]);
        for item in [ColumnTypeSpec::Json, ColumnTypeSpec::JsonBinary] {
            match bind_value(&val, &array_of(item)) {
                Value::Array(ArrayType::Json, Some(vals)) => {
                    let json = |v: serde_json::Value| Value::Json(Some(Box::new(v)));
                    assert_eq!(
                        *vals,
                        vec![
                            json(serde_json::json!({"a": 1})),
                            json(serde_json::json!(5)),
                            json(serde_json::json!(true)),
                            json(serde_json::json!(1.5)),
                            json(serde_json::json!("hello")),
                            json(serde_json::json!("123")),
                            json(serde_json::json!("null")),
                            json(serde_json::json!(r#"{"a": 1}"#)),
                            Value::Json(None),
                        ]
                    );
                }
                other => panic!("expected Array(Json), got {other:?}"),
            }
        }
        // A whole JSON column still takes a `str` field's JSON text.
        assert_eq!(
            bind_value(&s("123"), &ColumnTypeSpec::Json),
            Value::Json(Some(Box::new(serde_json::json!(123))))
        );
    }

    #[test]
    fn integer_elements_of_float_and_decimal_arrays_convert() {
        let val = rmpv::Value::Array(vec![1.into(), rmpv::Value::F64(2.5)]);
        assert_eq!(
            bind_value(&val, &array_of(ColumnTypeSpec::Double)),
            Value::Array(
                ArrayType::Double,
                Some(Box::new(vec![
                    Value::Double(Some(1.0)),
                    Value::Double(Some(2.5))
                ]))
            )
        );
        let val = rmpv::Value::Array(vec![1.into(), s("2.50")]);
        assert_eq!(
            bind_value(&val, &array_of(decimal_spec())),
            Value::Array(
                ArrayType::Decimal,
                Some(Box::new(vec![
                    Value::Decimal(Some(Box::new(Decimal::from(1)))),
                    Value::Decimal(Some(Box::new(Decimal::new(250, 2)))),
                ]))
            )
        );
    }

    #[test]
    fn decimal_exponent_string() {
        assert_eq!(
            bind_value(&s("1E+2"), &decimal_spec()),
            Value::Decimal(Some(Box::new(Decimal::from(100))))
        );
    }

    #[test]
    fn zero_decimals_with_any_exponent() {
        for zero in ["0E-30", "0E+40", "-0E-50", "0.000E-99"] {
            assert_eq!(parse_decimal_exact(zero), Some(Decimal::ZERO), "{zero}");
            assert_eq!(parse_decimal(zero), Some(Decimal::ZERO), "{zero}");
        }
        assert_eq!(parse_decimal_exact("0Eabc"), None);
        assert_eq!(parse_decimal("0Eabc"), None);
        assert_eq!(
            bind_value(&s("0E-30"), &decimal_spec()),
            Value::Decimal(Some(Box::new(Decimal::ZERO)))
        );
    }

    #[test]
    fn exact_decimals_with_any_exponent() {
        let tiny = Decimal::new(1, 27);
        let huge = Decimal::from_i128_with_scale(10i128.pow(27), 0);
        for (text, expected) in [
            ("100E-29", tiny),
            ("1000E-30", tiny),
            ("1.500E-27", Decimal::new(15, 28)),
            ("0.001E+30", huge),
            ("1E+2", Decimal::from(100)),
            // The mantissa itself does not fit, but the final value does.
            ("0.00000000000000000000000000001E+1", Decimal::new(1, 28)),
            (
                "1000000000000000000000000000000E-2",
                Decimal::from_i128_with_scale(10i128.pow(28), 0),
            ),
            ("-0.00000000000000000000000000001E+1", -Decimal::new(1, 28)),
            (
                "-1000000000000000000000000000000E-2",
                -Decimal::from_i128_with_scale(10i128.pow(28), 0),
            ),
            ("792281625142643375935439503350E-1", Decimal::MAX),
        ] {
            assert_eq!(parse_decimal_exact(text), Some(expected), "{text}");
        }
        // Would round, or overflow 96 bits.
        for text in [
            "1E-29",
            "1.001E-28",
            "1E+29",
            "8E+28",
            "79228162514264337593543950336E-1",
            "1E-99999999999999999999",
        ] {
            assert_eq!(parse_decimal_exact(text), None, "{text}");
        }
    }

    #[test]
    fn exact_decimal_extreme_exponents_do_not_overflow() {
        for text in [
            "1E-9223372036854775808",
            "1.0E-9223372036854775808",
            "10E+9223372036854775807",
        ] {
            assert_eq!(parse_decimal_exact(text), None, "{text}");
        }
        for text in ["0.0E-9223372036854775808", "0E+9223372036854775807"] {
            assert_eq!(parse_decimal_exact(text), Some(Decimal::ZERO), "{text}");
        }
    }

    #[test]
    fn exact_decimal_normalization_validates_the_whole_mantissa() {
        for text in ["1e", "1e1e1", "--1", "1.2.3E+1", "._0E+1", "__1E+2"] {
            assert_eq!(parse_decimal_exact(text), None, "{text}");
        }
        for (text, expected) in [
            ("+.5e1", Decimal::from(5)),
            ("1_000.50E-2", Decimal::new(10005, 3)),
            ("2.50", Decimal::new(250, 2)),
        ] {
            let value = parse_decimal_exact(text).unwrap();
            assert_eq!(value, expected, "{text}");
            assert_eq!(value.scale(), expected.scale(), "{text}");
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

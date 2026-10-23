//! PostgreSQL array types of columns with an array `db_type` override: the
//! type a filter value is cast to (`INTEGER[]` where the spec alone would say
//! `BIGINT[]`), and which values it can hold.

use sea_query::Value;

use crate::error::{QueryError, Result};

/// PostgreSQL array type of a column whose `db_type` is an array override,
/// for casting values to it. Size and precision modifiers are dropped
/// (`VARCHAR(5)[]` casts to `VARCHAR[]`) so the cast can neither truncate
/// nor round a value into a match.
///
/// The override is spliced into the SQL text. It comes from the model
/// definition, never from query input, and `pg_array_base` only passes
/// plain type names; an override it cannot read is an error rather than an
/// uncast value, which would fail later as a confusing "operator does not
/// exist".
pub(super) fn pg_array_type(db_type: &str) -> Result<String> {
    let db_type = db_type.trim();
    let base = pg_array_base(db_type).ok_or_else(|| {
        QueryError::UnsupportedOperation(format!(
            "db_type '{db_type}' is not a one-dimensional array of a plain type, \
             such as INTEGER[] or VARCHAR(50)[]"
        ))
    })?;
    if base.eq_ignore_ascii_case("JSON") {
        // json has no equality operator, so json[] values cannot compare.
        return Err(QueryError::UnsupportedOperation(
            "json[] columns have no equality operator; use jsonb[]".into(),
        ));
    }
    Ok(format!("{base}[]"))
}

/// Element type of an array DDL type (`VARCHAR(50)[]`) without modifiers.
/// `None` for non-array types and anything that is not a plain type name:
/// a dotted path of bare or double-quoted identifiers, or one of
/// PostgreSQL's multi-word type names (`DOUBLE PRECISION`).
fn pg_array_base(db_type: &str) -> Option<String> {
    let inner = db_type.trim().strip_suffix("[]")?;

    // FLOAT's precision picks the type: FLOAT(1..24) is real, FLOAT(25..53)
    // double precision, and bare FLOAT double precision.
    if let Some(precision) = pg_float_precision(inner) {
        return match precision.parse::<u8>() {
            Ok(1..=24) => Some("REAL".to_string()),
            Ok(25..=53) => Some("DOUBLE PRECISION".to_string()),
            _ => None,
        };
    }

    // Drop "(n)" / "(p,s)" wherever it sits ("TIMESTAMP(3) WITH TIME ZONE"),
    // but not inside a quoted identifier.
    let mut base = String::with_capacity(inner.len());
    let mut depth = 0u32;
    let mut quoted = false;
    for ch in inner.chars() {
        match ch {
            '"' if depth == 0 => {
                quoted = !quoted;
                base.push(ch);
            }
            _ if quoted => base.push(ch),
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            _ if depth == 0 => base.push(ch),
            _ => {}
        }
    }
    if depth != 0 || quoted {
        return None;
    }

    let base = base.trim();
    let base = if is_type_path(base) {
        base.to_string()
    } else {
        let words: Vec<&str> = base.split_whitespace().collect();
        if words.len() < 2 || !words.iter().all(|w| is_type_keyword(w)) {
            return None;
        }
        words.join(" ")
    };
    // A bare CHAR means CHAR(1); BPCHAR keeps the stored values' length.
    if matches!(
        base.to_ascii_uppercase().as_str(),
        "CHAR" | "CHARACTER" | "NCHAR" | "NATIONAL CHAR" | "NATIONAL CHARACTER"
    ) {
        return Some("BPCHAR".to_string());
    }
    Some(base)
}

/// A type name such as `int4`, `app.status` or `"App"."Status"`:
/// dot-separated identifiers, each bare or double-quoted.
fn is_type_path(name: &str) -> bool {
    let mut rest = name;
    loop {
        rest = if let Some(quoted) = rest.strip_prefix('"') {
            match quoted.find('"') {
                Some(end) if end > 0 => &quoted[end + 1..],
                _ => return false,
            }
        } else {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            if end == 0 || rest.starts_with(|c: char| c.is_ascii_digit()) {
                return false;
            }
            &rest[end..]
        };
        match rest.strip_prefix('.') {
            Some(next) => rest = next,
            None => return rest.is_empty(),
        }
    }
}

/// A word of PostgreSQL's multi-word type names (`DOUBLE PRECISION`,
/// `TIMESTAMP WITH TIME ZONE`, `INTERVAL DAY TO SECOND`, ...).
fn is_type_keyword(word: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "BIT",
        "CHAR",
        "CHARACTER",
        "DAY",
        "DOUBLE",
        "HOUR",
        "INTERVAL",
        "MINUTE",
        "MONTH",
        "NATIONAL",
        "PRECISION",
        "SECOND",
        "TIME",
        "TIMESTAMP",
        "TO",
        "VARYING",
        "WITH",
        "WITHOUT",
        "YEAR",
        "ZONE",
    ];
    KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word))
}

/// The text inside the parentheses of `FLOAT ( p )`, any case and spacing.
fn pg_float_precision(element: &str) -> Option<&str> {
    let element = element.trim();
    let (name, rest) = element.split_at_checked(5)?;
    if !name.eq_ignore_ascii_case("FLOAT") {
        return None;
    }
    let inside = rest.trim_start().strip_prefix('(')?.strip_suffix(')')?;
    Some(inside.trim())
}

/// Value range of a narrower integer array type (`db_type="SMALLINT[]"`).
/// `None` when the type holds every `i64`.
fn pg_int_range(base: &str) -> Option<(i64, i64)> {
    match base.to_ascii_uppercase().as_str() {
        "SMALLINT" | "INT2" => Some((i16::MIN.into(), i16::MAX.into())),
        "INTEGER" | "INT" | "INT4" => Some((i32::MIN.into(), i32::MAX.into())),
        _ => None,
    }
}

/// The first element of an array value that the column's array type cannot
/// hold, shown as text: an integer outside a `SMALLINT[]` / `INTEGER[]`
/// range, or a finite float that overflows or underflows `REAL[]`. Casting
/// such a value would fail the query ("smallint out of range", "value out
/// of range: overflow"). NaN and ±Infinity fit `REAL`, as they do in
/// PostgreSQL.
pub(super) fn out_of_range_element(value: &Value, array_type: &str) -> Option<String> {
    let base = array_type.strip_suffix("[]")?;
    let Value::Array(_, Some(elements)) = value else {
        return None;
    };
    let int_range = pg_int_range(base);
    let real = matches!(base.to_ascii_uppercase().as_str(), "REAL" | "FLOAT4");
    elements
        .iter()
        .find_map(|element| match (element, int_range) {
            (Value::BigInt(Some(n)), Some((min, max))) if !(min..=max).contains(n) => {
                Some(n.to_string())
            }
            #[allow(clippy::cast_possible_truncation)]
            (Value::Double(Some(x)), _)
                if real
                    && x.is_finite()
                    && (!(*x as f32).is_finite() || (*x as f32 == 0.0 && *x != 0.0)) =>
            {
                Some(x.to_string())
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use sea_query::value::ArrayType;

    use super::*;

    #[test]
    fn array_base_drops_modifiers() {
        assert_eq!(pg_array_base("VARCHAR(100)[]").as_deref(), Some("VARCHAR"));
        assert_eq!(
            pg_array_base("numeric(10, 2)[]").as_deref(),
            Some("numeric")
        );
        assert_eq!(pg_array_base("INTEGER[]").as_deref(), Some("INTEGER"));
        assert_eq!(
            pg_array_base("TIMESTAMP(3) WITH TIME ZONE[]").as_deref(),
            Some("TIMESTAMP WITH TIME ZONE")
        );
        assert_eq!(pg_array_base("CHAR(3)[]").as_deref(), Some("BPCHAR"));
        assert_eq!(pg_array_base("FLOAT(24)[]").as_deref(), Some("REAL"));
        assert_eq!(pg_array_base("float ( 1 )[]").as_deref(), Some("REAL"));
        for precise in ["FLOAT(25)[]", "FLOAT(53)[]"] {
            assert_eq!(
                pg_array_base(precise).as_deref(),
                Some("DOUBLE PRECISION"),
                "{precise}"
            );
        }
        assert_eq!(pg_array_base("FLOAT[]").as_deref(), Some("FLOAT"));
        assert_eq!(pg_array_base("FLOAT8[]").as_deref(), Some("FLOAT8"));
        for invalid in ["FLOAT(0)[]", "FLOAT(54)[]", "FLOAT(x)[]"] {
            assert_eq!(pg_array_base(invalid), None, "{invalid}");
        }
        assert_eq!(
            pg_array_base(r#""app"."status"[]"#).as_deref(),
            Some(r#""app"."status""#)
        );
        // Parentheses inside a quoted name are part of the name.
        assert_eq!(
            pg_array_base(r#""my(type)"[]"#).as_deref(),
            Some(r#""my(type)""#)
        );
        assert_eq!(
            pg_array_base("character varying(20)[]").as_deref(),
            Some("character varying")
        );
        assert_eq!(pg_array_base("NCHAR(3)[]").as_deref(), Some("BPCHAR"));
    }

    #[test]
    fn array_base_rejects_non_arrays_and_odd_text() {
        assert_eq!(pg_array_base("JSONB"), None);
        assert_eq!(pg_array_base("INT); DROP TABLE x; --[]"), None);
        assert_eq!(pg_array_base("VARCHAR(10[]"), None);
        assert_eq!(pg_array_base("[]"), None);
        // Several words are only a type name when PostgreSQL spells one so.
        assert_eq!(pg_array_base("INT OR TRUE[]"), None);
        for odd in [r#""app[]"#, r#"app"[]"#, r#"""[]"#, "1int[]", "app.[]"] {
            assert_eq!(pg_array_base(odd), None, "{odd}");
        }
    }

    #[test]
    fn unreadable_array_override_is_an_error() {
        assert_eq!(pg_array_type("INTEGER(4)[]").unwrap(), "INTEGER[]");
        assert_eq!(pg_array_type(" smallint[] ").unwrap(), "smallint[]");
        for db_type in [
            "INT[][]",
            "VARCHAR(10[]",
            "INT; DROP TABLE x[]",
            "INT OR TRUE[]",
            "[]",
        ] {
            let err = pg_array_type(db_type).unwrap_err().to_string();
            assert!(err.contains(db_type), "{db_type}: {err}");
        }
        let err = pg_array_type("JSON[]").unwrap_err().to_string();
        assert!(err.contains("use jsonb[]"), "{err}");
    }

    fn ints(ns: &[Option<i64>]) -> Value {
        let elements = ns.iter().map(|n| Value::BigInt(*n)).collect();
        Value::Array(ArrayType::BigInt, Some(Box::new(elements)))
    }

    fn floats(xs: &[f64]) -> Value {
        let elements = xs.iter().map(|x| Value::Double(Some(*x))).collect();
        Value::Array(ArrayType::Double, Some(Box::new(elements)))
    }

    #[test]
    fn narrow_integer_types_hold_only_their_range() {
        assert_eq!(
            out_of_range_element(&ints(&[Some(1), Some(-32768)]), "SMALLINT[]"),
            None
        );
        assert_eq!(
            out_of_range_element(&ints(&[Some(1), Some(100_000)]), "smallint[]").as_deref(),
            Some("100000")
        );
        let over_i32 = ints(&[Some(i64::from(i32::MAX) + 1)]);
        assert!(out_of_range_element(&over_i32, "INTEGER[]").is_some());
        assert!(out_of_range_element(&over_i32, "INT4[]").is_some());
        assert_eq!(out_of_range_element(&over_i32, "BIGINT[]"), None);
        // NULL elements are never out of range.
        assert_eq!(out_of_range_element(&ints(&[None]), "SMALLINT[]"), None);
    }

    #[test]
    fn real_holds_its_range_and_non_finite_values() {
        let fits = floats(&[3.0e38, -3.0e38, 0.0, -0.0, 1.5, 1e-40]);
        assert_eq!(out_of_range_element(&fits, "REAL[]"), None);
        assert_eq!(out_of_range_element(&fits, "float4[]"), None);
        // PostgreSQL's real holds NaN and the infinities.
        let non_finite = floats(&[f64::NAN, f64::INFINITY, f64::NEG_INFINITY]);
        assert_eq!(out_of_range_element(&non_finite, "REAL[]"), None);
        for x in [1e300, -1e300, 1e-300] {
            assert!(out_of_range_element(&floats(&[1.0, x]), "REAL[]").is_some());
            assert_eq!(
                out_of_range_element(&floats(&[x]), "DOUBLE PRECISION[]"),
                None
            );
        }

        // FLOAT(24) is real, so it gets the same check.
        let real = pg_array_type("FLOAT(24)[]").unwrap();
        assert_eq!(real, "REAL[]");
        assert!(out_of_range_element(&floats(&[1e300]), &real).is_some());
    }
}

//! Shared helpers for parameter binding across all backends

use crate::error::{DriverError, Result};
use sea_query::Value;

pub fn cast_u64_to_i64(value: u64, db: &str) -> Result<i64> {
    if value > i64::MAX as u64 {
        return Err(DriverError::ExecutionError(format!(
            "Parameter out of range for {}: {}",
            db, value
        )));
    }
    Ok(value as i64)
}

pub fn unsupported_param(db: &str, value: &Value) -> DriverError {
    DriverError::ExecutionError(format!(
        "Unsupported parameter type for {}: {:?}",
        db, value
    ))
}

/// Convert a sea_query `Value` to a `serde_json::Value` for JSON storage.
/// Used by MySQL/SQLite to serialize array elements.
fn sea_value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Bool(Some(b)) => serde_json::Value::Bool(*b),
        Value::TinyInt(Some(v)) => serde_json::Value::Number((*v).into()),
        Value::SmallInt(Some(v)) => serde_json::Value::Number((*v).into()),
        Value::Int(Some(v)) => serde_json::Value::Number((*v).into()),
        Value::BigInt(Some(v)) => serde_json::Value::Number((*v).into()),
        Value::Float(Some(v)) => serde_json::Number::from_f64(f64::from(*v))
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Double(Some(v)) => serde_json::Number::from_f64(*v)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(Some(s)) => serde_json::Value::String(s.as_ref().to_string()),
        Value::Uuid(Some(u)) => serde_json::Value::String(u.to_string()),
        Value::Decimal(Some(d)) => serde_json::Value::String(d.to_string()),
        Value::Json(Some(j)) => j.as_ref().clone(),
        Value::ChronoDateTime(Some(dt)) => {
            serde_json::Value::String(dt.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
        }
        Value::ChronoDateTimeUtc(Some(dt)) => serde_json::Value::String(dt.to_rfc3339()),
        Value::ChronoDate(Some(d)) => serde_json::Value::String(d.format("%Y-%m-%d").to_string()),
        Value::ChronoTime(Some(t)) => {
            serde_json::Value::String(t.format("%H:%M:%S%.f").to_string())
        }
        _ => serde_json::Value::Null,
    }
}

/// Convert a sea_query `Value::Array` to a `serde_json::Value::Array`.
/// Used by MySQL/SQLite which store arrays as JSON.
pub fn array_values_to_json(vals: &[Value]) -> serde_json::Value {
    serde_json::Value::Array(vals.iter().map(sea_value_to_json).collect())
}

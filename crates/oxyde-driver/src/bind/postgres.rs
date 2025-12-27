//! PostgreSQL parameter binding

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::error::{DriverError, Result};
use sea_query::{ArrayType, Value};

pub type PgQuery<'q> = sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>;

pub fn bind_postgres<'q>(mut query: PgQuery<'q>, params: &'q [Value]) -> Result<PgQuery<'q>> {
    for value in params {
        query = bind_postgres_value(query, value)?;
    }
    Ok(query)
}

pub fn bind_postgres_value<'q>(query: PgQuery<'q>, value: &'q Value) -> Result<PgQuery<'q>> {
    let query = match value {
        Value::Bool(Some(v)) => query.bind(*v),
        Value::Bool(None) => query.bind(Option::<bool>::None),
        Value::TinyInt(Some(v)) => query.bind(*v),
        Value::TinyInt(None) => query.bind(Option::<i8>::None),
        Value::SmallInt(Some(v)) => query.bind(*v),
        Value::SmallInt(None) => query.bind(Option::<i16>::None),
        Value::Int(Some(v)) => query.bind(*v),
        Value::Int(None) => query.bind(Option::<i32>::None),
        Value::BigInt(Some(v)) => query.bind(*v),
        Value::BigInt(None) => query.bind(Option::<i64>::None),
        Value::TinyUnsigned(Some(v)) => query.bind(*v as i16),
        Value::TinyUnsigned(None) => query.bind(Option::<i16>::None),
        Value::SmallUnsigned(Some(v)) => query.bind(*v as i32),
        Value::SmallUnsigned(None) => query.bind(Option::<i32>::None),
        Value::Unsigned(Some(v)) => query.bind(cast_u64_to_i64((*v).into(), "Postgres")?),
        Value::Unsigned(None) => query.bind(Option::<i64>::None),
        Value::BigUnsigned(Some(v)) => query.bind(cast_u64_to_i64(*v, "Postgres")?),
        Value::BigUnsigned(None) => query.bind(Option::<i64>::None),
        Value::Float(Some(v)) => query.bind(*v),
        Value::Float(None) => query.bind(Option::<f32>::None),
        Value::Double(Some(v)) => query.bind(*v),
        Value::Double(None) => query.bind(Option::<f64>::None),
        Value::String(Some(s)) => query.bind(s.as_ref().as_str()),
        Value::String(None) => query.bind(Option::<String>::None),
        Value::Char(Some(c)) => query.bind(c.to_string()),
        Value::Char(None) => query.bind(Option::<String>::None),
        Value::Bytes(Some(bytes)) => query.bind(bytes.as_ref().as_slice()),
        Value::Bytes(None) => query.bind(Option::<Vec<u8>>::None),
        // Chrono types
        Value::ChronoDateTime(Some(dt)) => query.bind(**dt),
        Value::ChronoDateTime(None) => query.bind(Option::<NaiveDateTime>::None),
        Value::ChronoDate(Some(d)) => query.bind(**d),
        Value::ChronoDate(None) => query.bind(Option::<NaiveDate>::None),
        Value::ChronoTime(Some(t)) => query.bind(**t),
        Value::ChronoTime(None) => query.bind(Option::<NaiveTime>::None),
        // UUID
        Value::Uuid(Some(u)) => query.bind(**u),
        Value::Uuid(None) => query.bind(Option::<Uuid>::None),
        // JSON
        Value::Json(Some(j)) => query.bind(j.as_ref().clone()),
        Value::Json(None) => query.bind(Option::<serde_json::Value>::None),
        // Decimal
        Value::Decimal(Some(d)) => query.bind(**d),
        Value::Decimal(None) => query.bind(Option::<Decimal>::None),
        // Array (PostgreSQL native)
        Value::Array(array_type, Some(arr)) => bind_array(query, array_type, arr.as_ref())?,
        Value::Array(array_type, None) => bind_null_array(query, array_type),
        #[allow(unreachable_patterns)]
        other => return Err(unsupported_param("Postgres", other)),
    };
    Ok(query)
}

fn cast_u64_to_i64(value: u64, db: &str) -> Result<i64> {
    if value > i64::MAX as u64 {
        return Err(DriverError::ExecutionError(format!(
            "Parameter out of range for {}: {}",
            db, value
        )));
    }
    Ok(value as i64)
}

fn unsupported_param(db: &str, value: &Value) -> DriverError {
    DriverError::ExecutionError(format!(
        "Unsupported parameter type for {}: {:?}",
        db, value
    ))
}

/// Bind NULL array with correct type for PostgreSQL
fn bind_null_array<'q>(query: PgQuery<'q>, array_type: &ArrayType) -> PgQuery<'q> {
    match array_type {
        ArrayType::Bool => query.bind(Option::<Vec<bool>>::None),
        ArrayType::TinyInt => query.bind(Option::<Vec<i8>>::None),
        ArrayType::SmallInt => query.bind(Option::<Vec<i16>>::None),
        ArrayType::Int => query.bind(Option::<Vec<i32>>::None),
        ArrayType::BigInt => query.bind(Option::<Vec<i64>>::None),
        ArrayType::TinyUnsigned | ArrayType::SmallUnsigned => query.bind(Option::<Vec<i32>>::None),
        ArrayType::Unsigned | ArrayType::BigUnsigned => query.bind(Option::<Vec<i64>>::None),
        ArrayType::Float => query.bind(Option::<Vec<f32>>::None),
        ArrayType::Double => query.bind(Option::<Vec<f64>>::None),
        ArrayType::String | ArrayType::Char => query.bind(Option::<Vec<String>>::None),
        ArrayType::Bytes => query.bind(Option::<Vec<Vec<u8>>>::None),
        ArrayType::Uuid => query.bind(Option::<Vec<Uuid>>::None),
        ArrayType::Json => query.bind(Option::<Vec<serde_json::Value>>::None),
        ArrayType::ChronoDate => query.bind(Option::<Vec<NaiveDate>>::None),
        ArrayType::ChronoTime => query.bind(Option::<Vec<NaiveTime>>::None),
        ArrayType::ChronoDateTime => query.bind(Option::<Vec<NaiveDateTime>>::None),
        ArrayType::Decimal => query.bind(Option::<Vec<Decimal>>::None),
        // Fallback for any other types
        #[allow(unreachable_patterns)]
        _ => query.bind(Option::<Vec<i32>>::None),
    }
}

/// Bind empty array with correct type for PostgreSQL
fn bind_empty_array<'q>(query: PgQuery<'q>, array_type: &ArrayType) -> PgQuery<'q> {
    match array_type {
        ArrayType::Bool => query.bind(Vec::<bool>::new()),
        ArrayType::TinyInt => query.bind(Vec::<i8>::new()),
        ArrayType::SmallInt => query.bind(Vec::<i16>::new()),
        ArrayType::Int => query.bind(Vec::<i32>::new()),
        ArrayType::BigInt => query.bind(Vec::<i64>::new()),
        ArrayType::TinyUnsigned | ArrayType::SmallUnsigned => query.bind(Vec::<i32>::new()),
        ArrayType::Unsigned | ArrayType::BigUnsigned => query.bind(Vec::<i64>::new()),
        ArrayType::Float => query.bind(Vec::<f32>::new()),
        ArrayType::Double => query.bind(Vec::<f64>::new()),
        ArrayType::String | ArrayType::Char => query.bind(Vec::<String>::new()),
        ArrayType::Bytes => query.bind(Vec::<Vec<u8>>::new()),
        ArrayType::Uuid => query.bind(Vec::<Uuid>::new()),
        ArrayType::Json => query.bind(Vec::<serde_json::Value>::new()),
        ArrayType::ChronoDate => query.bind(Vec::<NaiveDate>::new()),
        ArrayType::ChronoTime => query.bind(Vec::<NaiveTime>::new()),
        ArrayType::ChronoDateTime => query.bind(Vec::<NaiveDateTime>::new()),
        ArrayType::Decimal => query.bind(Vec::<Decimal>::new()),
        // Fallback for any other types
        #[allow(unreachable_patterns)]
        _ => query.bind(Vec::<i32>::new()),
    }
}

/// Bind array values for PostgreSQL
fn bind_array<'q>(
    query: PgQuery<'q>,
    array_type: &ArrayType,
    values: &'q [Value],
) -> Result<PgQuery<'q>> {
    // Use ArrayType for empty arrays
    if values.is_empty() {
        return Ok(bind_empty_array(query, array_type));
    }

    // Convert array based on element type
    match &values[0] {
        Value::Int(_) => {
            let arr: Vec<Option<i32>> = values
                .iter()
                .map(|v| match v {
                    Value::Int(i) => *i,
                    _ => None,
                })
                .collect();
            Ok(query.bind(arr))
        }
        Value::BigInt(_) => {
            let arr: Vec<Option<i64>> = values
                .iter()
                .map(|v| match v {
                    Value::BigInt(i) => *i,
                    _ => None,
                })
                .collect();
            Ok(query.bind(arr))
        }
        Value::String(_) => {
            let arr: Vec<Option<String>> = values
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.as_ref().map(|b| b.as_ref().clone()),
                    _ => None,
                })
                .collect();
            Ok(query.bind(arr))
        }
        Value::Double(_) => {
            let arr: Vec<Option<f64>> = values
                .iter()
                .map(|v| match v {
                    Value::Double(d) => *d,
                    _ => None,
                })
                .collect();
            Ok(query.bind(arr))
        }
        Value::Bool(_) => {
            let arr: Vec<Option<bool>> = values
                .iter()
                .map(|v| match v {
                    Value::Bool(b) => *b,
                    _ => None,
                })
                .collect();
            Ok(query.bind(arr))
        }
        Value::Uuid(_) => {
            let arr: Vec<Option<Uuid>> = values
                .iter()
                .map(|v| match v {
                    Value::Uuid(u) => u.as_ref().map(|b| **b),
                    _ => None,
                })
                .collect();
            Ok(query.bind(arr))
        }
        _ => Err(DriverError::ExecutionError(format!(
            "Unsupported array element type for PostgreSQL: {:?}",
            values[0]
        ))),
    }
}

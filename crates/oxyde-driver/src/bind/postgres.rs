//! PostgreSQL parameter binding

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use sea_query::value::ArrayType;
use uuid::Uuid;

use crate::error::Result;
use sea_query::Value;

use super::common::{cast_u64_to_i64, unsupported_param};

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
        Value::ChronoDateTimeUtc(Some(dt)) => query.bind(**dt),
        Value::ChronoDateTimeUtc(None) => query.bind(Option::<DateTime<Utc>>::None),
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
        // Arrays (native PostgreSQL arrays)
        Value::Array(arr_type, Some(vals)) => return bind_pg_array(query, arr_type, vals),
        Value::Array(arr_type, None) => return bind_pg_array_null(query, arr_type),
        #[allow(unreachable_patterns)]
        other => return Err(unsupported_param("Postgres", other)),
    };
    Ok(query)
}

/// Bind a non-null PostgreSQL array parameter.
fn bind_pg_array<'q>(
    query: PgQuery<'q>,
    arr_type: &ArrayType,
    vals: &'q [Value],
) -> Result<PgQuery<'q>> {
    let query = match arr_type {
        ArrayType::Bool => {
            let v: Vec<Option<bool>> = vals
                .iter()
                .map(|v| match v {
                    Value::Bool(v) => *v,
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::BigInt => {
            let v: Vec<Option<i64>> = vals
                .iter()
                .map(|v| match v {
                    Value::BigInt(v) => *v,
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::Double => {
            let v: Vec<Option<f64>> = vals
                .iter()
                .map(|v| match v {
                    Value::Double(v) => *v,
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::String => {
            let v: Vec<Option<String>> = vals
                .iter()
                .map(|v| match v {
                    Value::String(Some(s)) => Some(s.as_ref().to_string()),
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::Uuid => {
            let v: Vec<Option<Uuid>> = vals
                .iter()
                .map(|v| match v {
                    Value::Uuid(Some(u)) => Some(**u),
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::Decimal => {
            let v: Vec<Option<Decimal>> = vals
                .iter()
                .map(|v| match v {
                    Value::Decimal(Some(d)) => Some(**d),
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::ChronoDateTime => {
            let v: Vec<Option<NaiveDateTime>> = vals
                .iter()
                .map(|v| match v {
                    Value::ChronoDateTime(Some(dt)) => Some(**dt),
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::ChronoDateTimeUtc => {
            let v: Vec<Option<DateTime<Utc>>> = vals
                .iter()
                .map(|v| match v {
                    Value::ChronoDateTimeUtc(Some(dt)) => Some(**dt),
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::ChronoDate => {
            let v: Vec<Option<NaiveDate>> = vals
                .iter()
                .map(|v| match v {
                    Value::ChronoDate(Some(d)) => Some(**d),
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::ChronoTime => {
            let v: Vec<Option<NaiveTime>> = vals
                .iter()
                .map(|v| match v {
                    Value::ChronoTime(Some(t)) => Some(**t),
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        ArrayType::Json => {
            let v: Vec<Option<serde_json::Value>> = vals
                .iter()
                .map(|v| match v {
                    Value::Json(Some(j)) => Some(j.as_ref().clone()),
                    _ => None,
                })
                .collect();
            query.bind(v)
        }
        #[allow(unreachable_patterns)]
        _ => {
            return Err(unsupported_param(
                "Postgres",
                &Value::Array(arr_type.clone(), None),
            ))
        }
    };
    Ok(query)
}

/// Bind a NULL PostgreSQL array parameter with the correct type OID.
fn bind_pg_array_null<'q>(query: PgQuery<'q>, arr_type: &ArrayType) -> Result<PgQuery<'q>> {
    let query = match arr_type {
        ArrayType::Bool => query.bind(Option::<Vec<bool>>::None),
        ArrayType::BigInt => query.bind(Option::<Vec<i64>>::None),
        ArrayType::Double => query.bind(Option::<Vec<f64>>::None),
        ArrayType::String => query.bind(Option::<Vec<String>>::None),
        ArrayType::Uuid => query.bind(Option::<Vec<Uuid>>::None),
        ArrayType::Decimal => query.bind(Option::<Vec<Decimal>>::None),
        ArrayType::ChronoDateTime => query.bind(Option::<Vec<NaiveDateTime>>::None),
        ArrayType::ChronoDateTimeUtc => query.bind(Option::<Vec<DateTime<Utc>>>::None),
        ArrayType::ChronoDate => query.bind(Option::<Vec<NaiveDate>>::None),
        ArrayType::ChronoTime => query.bind(Option::<Vec<NaiveTime>>::None),
        ArrayType::Json => query.bind(Option::<Vec<serde_json::Value>>::None),
        #[allow(unreachable_patterns)]
        _ => {
            return Err(unsupported_param(
                "Postgres",
                &Value::Array(arr_type.clone(), None),
            ))
        }
    };
    Ok(query)
}

//! PostgreSQL parameter binding

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::error::Result;
use sea_query::{ArrayType, Value};

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
        // PostgreSQL arrays
        Value::Array(ty, val) => bind_pg_array(query, ty, val)?,
        #[allow(unreachable_patterns)]
        other => return Err(unsupported_param("Postgres", other)),
    };
    Ok(query)
}

/// Bind a sea_query Array value to a PostgreSQL query parameter.
fn bind_pg_array<'q>(
    query: PgQuery<'q>,
    ty: &ArrayType,
    val: &'q Option<Box<Vec<Value>>>,
) -> Result<PgQuery<'q>> {
    macro_rules! bind_array {
        ($query:expr, $val:expr, $arr_ty:ident, $rust_ty:ty, $extract:expr) => {
            match $val {
                Some(vec) => {
                    let items: Vec<Option<$rust_ty>> =
                        vec.as_ref().iter().map($extract).collect();
                    Ok($query.bind(items))
                }
                None => Ok($query.bind(Option::<Vec<$rust_ty>>::None)),
            }
        };
    }

    match ty {
        ArrayType::String => bind_array!(query, val, String, String, |v| match v {
            Value::String(Some(s)) => Some(s.as_ref().to_string()),
            _ => None,
        }),
        ArrayType::BigInt => bind_array!(query, val, BigInt, i64, |v| match v {
            Value::BigInt(Some(n)) => Some(*n),
            _ => None,
        }),
        ArrayType::Int => bind_array!(query, val, Int, i32, |v| match v {
            Value::Int(Some(n)) => Some(*n),
            _ => None,
        }),
        ArrayType::SmallInt => bind_array!(query, val, SmallInt, i16, |v| match v {
            Value::SmallInt(Some(n)) => Some(*n),
            _ => None,
        }),
        ArrayType::Float => bind_array!(query, val, Float, f32, |v| match v {
            Value::Float(Some(n)) => Some(*n),
            _ => None,
        }),
        ArrayType::Double => bind_array!(query, val, Double, f64, |v| match v {
            Value::Double(Some(n)) => Some(*n),
            _ => None,
        }),
        ArrayType::Bool => bind_array!(query, val, Bool, bool, |v| match v {
            Value::Bool(Some(b)) => Some(*b),
            _ => None,
        }),
        ArrayType::Uuid => bind_array!(query, val, Uuid, Uuid, |v| match v {
            Value::Uuid(Some(u)) => Some(**u),
            _ => None,
        }),
        ArrayType::Json => bind_array!(
            query,
            val,
            Json,
            serde_json::Value,
            |v| match v {
                Value::Json(Some(j)) => Some(j.as_ref().clone()),
                _ => None,
            }
        ),
        ArrayType::ChronoDateTime => bind_array!(
            query,
            val,
            ChronoDateTime,
            NaiveDateTime,
            |v| match v {
                Value::ChronoDateTime(Some(dt)) => Some(**dt),
                _ => None,
            }
        ),
        ArrayType::ChronoDateTimeUtc => bind_array!(
            query,
            val,
            ChronoDateTimeUtc,
            DateTime<Utc>,
            |v| match v {
                Value::ChronoDateTimeUtc(Some(dt)) => Some(**dt),
                _ => None,
            }
        ),
        ArrayType::ChronoDate => bind_array!(query, val, ChronoDate, NaiveDate, |v| match v {
            Value::ChronoDate(Some(d)) => Some(**d),
            _ => None,
        }),
        ArrayType::ChronoTime => bind_array!(query, val, ChronoTime, NaiveTime, |v| match v {
            Value::ChronoTime(Some(t)) => Some(**t),
            _ => None,
        }),
        ArrayType::Decimal => bind_array!(query, val, Decimal, Decimal, |v| match v {
            Value::Decimal(Some(d)) => Some(**d),
            _ => None,
        }),
        _ => {
            // Fallback for unsupported array types: bind as string array
            match val {
                Some(vec) => {
                    let items: Vec<Option<String>> = vec
                        .as_ref()
                        .iter()
                        .map(|v| match v {
                            Value::String(Some(s)) => Some(s.as_ref().to_string()),
                            _ => None,
                        })
                        .collect();
                    Ok(query.bind(items))
                }
                None => Ok(query.bind(Option::<Vec<String>>::None)),
            }
        }
    }
}

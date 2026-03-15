//! MySQL parameter binding

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use rust_decimal::Decimal;

use crate::error::Result;
use sea_query::Value;

use super::common::{array_values_to_json, unsupported_param};

pub type MySqlQuery<'q> = sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>;

pub fn bind_mysql<'q>(mut query: MySqlQuery<'q>, params: &'q [Value]) -> Result<MySqlQuery<'q>> {
    for value in params {
        query = bind_mysql_value(query, value)?;
    }
    Ok(query)
}

pub fn bind_mysql_value<'q>(query: MySqlQuery<'q>, value: &'q Value) -> Result<MySqlQuery<'q>> {
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
        Value::TinyUnsigned(Some(v)) => query.bind(*v),
        Value::TinyUnsigned(None) => query.bind(Option::<u8>::None),
        Value::SmallUnsigned(Some(v)) => query.bind(*v),
        Value::SmallUnsigned(None) => query.bind(Option::<u16>::None),
        Value::Unsigned(Some(v)) => query.bind(*v),
        Value::Unsigned(None) => query.bind(Option::<u32>::None),
        Value::BigUnsigned(Some(v)) => query.bind(*v),
        Value::BigUnsigned(None) => query.bind(Option::<u64>::None),
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
        // Chrono types. tz-aware normalized to naive UTC
        Value::ChronoDateTimeUtc(Some(dt)) => query.bind(dt.naive_utc()),
        Value::ChronoDateTimeUtc(None) => query.bind(Option::<NaiveDateTime>::None),
        Value::ChronoDateTime(Some(dt)) => query.bind(**dt),
        Value::ChronoDateTime(None) => query.bind(Option::<NaiveDateTime>::None),
        Value::ChronoDate(Some(d)) => query.bind(**d),
        Value::ChronoDate(None) => query.bind(Option::<NaiveDate>::None),
        Value::ChronoTime(Some(t)) => query.bind(**t),
        Value::ChronoTime(None) => query.bind(Option::<NaiveTime>::None),
        // UUID (stored as CHAR(36) in MySQL)
        Value::Uuid(Some(u)) => query.bind(u.to_string()),
        Value::Uuid(None) => query.bind(Option::<String>::None),
        // JSON (native JSON type in MySQL 5.7+)
        Value::Json(Some(j)) => query.bind(j.as_ref().clone()),
        Value::Json(None) => query.bind(Option::<serde_json::Value>::None),
        // Decimal
        Value::Decimal(Some(d)) => query.bind(**d),
        Value::Decimal(None) => query.bind(Option::<Decimal>::None),
        // Arrays (stored as JSON in MySQL)
        Value::Array(_, Some(vals)) => query.bind(array_values_to_json(vals)),
        Value::Array(_, None) => query.bind(Option::<serde_json::Value>::None),
        #[allow(unreachable_patterns)]
        other => return Err(unsupported_param("MySQL", other)),
    };
    Ok(query)
}

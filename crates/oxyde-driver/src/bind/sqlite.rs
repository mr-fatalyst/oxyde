//! SQLite parameter binding

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use crate::error::Result;
use sea_query::Value;

use super::common::{cast_u64_to_i64, unsupported_param};

// Note: SQLite stores UUID, JSON, Decimal as TEXT
// rust_decimal and uuid crates are not needed here since we serialize to string

pub type SqliteQuery<'q> = sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>;

pub fn bind_sqlite<'q>(mut query: SqliteQuery<'q>, params: &'q [Value]) -> Result<SqliteQuery<'q>> {
    for value in params {
        query = bind_sqlite_value(query, value)?;
    }
    Ok(query)
}

pub fn bind_sqlite_value<'q>(query: SqliteQuery<'q>, value: &'q Value) -> Result<SqliteQuery<'q>> {
    let query = match value {
        Value::Bool(Some(v)) => query.bind(*v),
        Value::Bool(None) => query.bind(Option::<bool>::None),
        Value::TinyInt(Some(v)) => query.bind(*v as i64),
        Value::TinyInt(None) => query.bind(Option::<i64>::None),
        Value::SmallInt(Some(v)) => query.bind(*v as i64),
        Value::SmallInt(None) => query.bind(Option::<i64>::None),
        Value::Int(Some(v)) => query.bind(*v as i64),
        Value::Int(None) => query.bind(Option::<i64>::None),
        Value::BigInt(Some(v)) => query.bind(*v),
        Value::BigInt(None) => query.bind(Option::<i64>::None),
        Value::TinyUnsigned(Some(v)) => query.bind(*v as i64),
        Value::TinyUnsigned(None) => query.bind(Option::<i64>::None),
        Value::SmallUnsigned(Some(v)) => query.bind(*v as i64),
        Value::SmallUnsigned(None) => query.bind(Option::<i64>::None),
        Value::Unsigned(Some(v)) => query.bind(cast_u64_to_i64((*v).into(), "SQLite")?),
        Value::Unsigned(None) => query.bind(Option::<i64>::None),
        Value::BigUnsigned(Some(v)) => query.bind(cast_u64_to_i64(*v, "SQLite")?),
        Value::BigUnsigned(None) => query.bind(Option::<i64>::None),
        Value::Float(Some(v)) => query.bind(*v as f64),
        Value::Float(None) => query.bind(Option::<f64>::None),
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
        // UUID (stored as TEXT in SQLite)
        Value::Uuid(Some(u)) => query.bind(u.to_string()),
        Value::Uuid(None) => query.bind(Option::<String>::None),
        // JSON (stored as TEXT in SQLite)
        Value::Json(Some(j)) => query.bind(j.to_string()),
        Value::Json(None) => query.bind(Option::<String>::None),
        // Decimal (stored as TEXT in SQLite for precision)
        Value::Decimal(Some(d)) => query.bind(d.to_string()),
        Value::Decimal(None) => query.bind(Option::<String>::None),
        #[allow(unreachable_patterns)]
        other => return Err(unsupported_param("SQLite", other)),
    };
    Ok(query)
}

//! MySQL parameter binding

use crate::error::{DriverError, Result};
use sea_query::Value;

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
        #[allow(unreachable_patterns)]
        other => return Err(unsupported_param("MySQL", other)),
    };
    Ok(query)
}

fn unsupported_param(db: &str, value: &Value) -> DriverError {
    DriverError::ExecutionError(format!(
        "Unsupported parameter type for {}: {:?}",
        db, value
    ))
}

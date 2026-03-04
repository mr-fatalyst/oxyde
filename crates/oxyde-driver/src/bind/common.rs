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

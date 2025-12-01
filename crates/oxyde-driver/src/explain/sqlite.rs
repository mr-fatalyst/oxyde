//! SQLite EXPLAIN functionality

use crate::error::{DriverError, Result};
use crate::explain::postgres::ExplainFormat;

pub fn build_sqlite_explain_sql(
    sql: &str,
    options: &crate::explain::postgres::ExplainOptions,
) -> Result<String> {
    if options.analyze {
        return Err(DriverError::ExecutionError(
            "SQLite EXPLAIN does not support ANALYZE".into(),
        ));
    }
    if matches!(options.format, ExplainFormat::Json) {
        return Err(DriverError::ExecutionError(
            "SQLite EXPLAIN does not support FORMAT JSON".into(),
        ));
    }
    Ok(format!("EXPLAIN QUERY PLAN {}", sql))
}

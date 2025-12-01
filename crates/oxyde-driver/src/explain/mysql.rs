//! MySQL EXPLAIN functionality
//!
//! MySQL supports:
//! - `EXPLAIN SELECT ...` - basic explain
//! - `EXPLAIN FORMAT=JSON SELECT ...` - JSON format (MySQL 5.6+)
//! - `EXPLAIN ANALYZE SELECT ...` - with execution stats (MySQL 8.0.18+)

use crate::error::Result;
use crate::explain::postgres::{ExplainFormat, ExplainOptions};
use std::collections::HashMap;

pub fn build_mysql_explain_sql(sql: &str, options: &ExplainOptions) -> Result<String> {
    // MySQL uses FORMAT=value syntax (with =), unlike PostgreSQL's FORMAT value
    let mut parts: Vec<&str> = vec!["EXPLAIN"];

    if options.analyze {
        parts.push("ANALYZE");
    }

    if matches!(options.format, ExplainFormat::Json) {
        parts.push("FORMAT=JSON");
    }

    parts.push(sql);
    Ok(parts.join(" "))
}

/// Extract JSON plan from MySQL EXPLAIN FORMAT=JSON result
/// MySQL returns a single row with "EXPLAIN" column containing JSON string
pub fn extract_mysql_json_plan(rows: Vec<HashMap<String, serde_json::Value>>) -> serde_json::Value {
    rows.into_iter()
        .find_map(|row| {
            // MySQL uses "EXPLAIN" as column name for JSON output
            row.get("EXPLAIN").cloned().or_else(|| {
                // Fallback: try first column value
                row.into_values().next()
            })
        })
        .and_then(|value| {
            // If it's a string, try to parse as JSON
            if let serde_json::Value::String(s) = &value {
                serde_json::from_str(s).ok()
            } else {
                Some(value)
            }
        })
        .unwrap_or(serde_json::Value::Null)
}

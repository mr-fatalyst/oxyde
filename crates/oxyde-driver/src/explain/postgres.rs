//! PostgreSQL EXPLAIN functionality

use crate::error::Result;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplainFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy)]
pub struct ExplainOptions {
    pub analyze: bool,
    pub format: ExplainFormat,
}

pub fn build_postgres_explain_sql(sql: &str, options: &ExplainOptions) -> Result<String> {
    let mut clauses: Vec<String> = Vec::new();
    if options.analyze {
        clauses.push("ANALYZE TRUE".into());
    }
    if matches!(options.format, ExplainFormat::Json) {
        clauses.push("FORMAT JSON".into());
    }
    if clauses.is_empty() {
        Ok(format!("EXPLAIN {}", sql))
    } else {
        Ok(format!("EXPLAIN ({}) {}", clauses.join(", "), sql))
    }
}

pub fn extract_postgres_json_plan(
    rows: Vec<HashMap<String, serde_json::Value>>,
) -> serde_json::Value {
    rows.into_iter()
        .find_map(|row| row.get("QUERY PLAN").cloned())
        .unwrap_or(serde_json::Value::Null)
}

pub fn extract_text_plan(
    rows: Vec<HashMap<String, serde_json::Value>>,
    column: &str,
) -> serde_json::Value {
    let lines = rows
        .into_iter()
        .filter_map(|row| row.get(column).cloned())
        .map(|value| match value {
            serde_json::Value::String(s) => serde_json::Value::String(s),
            other => serde_json::Value::String(other.to_string()),
        })
        .collect();
    serde_json::Value::Array(lines)
}

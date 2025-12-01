//! MySQL backend tests

#[cfg(test)]
mod tests {
    use crate::explain::mysql::{build_mysql_explain_sql, extract_mysql_json_plan};
    use crate::explain::postgres::{ExplainFormat, ExplainOptions};

    #[test]
    fn test_explain_sql_basic() {
        let options = ExplainOptions {
            analyze: false,
            format: ExplainFormat::Text,
        };
        let sql = build_mysql_explain_sql("SELECT * FROM users", &options).unwrap();
        assert!(sql.contains("EXPLAIN"));
        assert!(sql.contains("SELECT * FROM users"));
    }

    #[test]
    fn test_explain_sql_analyze() {
        let options = ExplainOptions {
            analyze: true,
            format: ExplainFormat::Text,
        };
        let sql = build_mysql_explain_sql("SELECT * FROM users", &options).unwrap();
        assert!(sql.contains("ANALYZE"));
    }

    #[test]
    fn test_explain_sql_format_json() {
        let options = ExplainOptions {
            analyze: false,
            format: ExplainFormat::Json,
        };
        let sql = build_mysql_explain_sql("SELECT * FROM users", &options).unwrap();
        assert!(
            sql.contains("FORMAT=JSON"),
            "Should contain FORMAT=JSON, got: {}",
            sql
        );
        assert!(sql.contains("SELECT * FROM users"));
    }

    #[test]
    fn test_explain_sql_analyze_json() {
        let options = ExplainOptions {
            analyze: true,
            format: ExplainFormat::Json,
        };
        let sql = build_mysql_explain_sql("SELECT * FROM users", &options).unwrap();
        assert!(
            sql.contains("ANALYZE"),
            "Should contain ANALYZE, got: {}",
            sql
        );
        assert!(
            sql.contains("FORMAT=JSON"),
            "Should contain FORMAT=JSON, got: {}",
            sql
        );
    }

    #[test]
    fn test_extract_mysql_json_plan() {
        use std::collections::HashMap;

        let json_str = r#"{"query_block": {"select_id": 1}}"#;
        let mut row = HashMap::new();
        row.insert(
            "EXPLAIN".to_string(),
            serde_json::Value::String(json_str.to_string()),
        );

        let result = extract_mysql_json_plan(vec![row]);
        assert!(result.is_object(), "Should be parsed JSON object");
        assert!(result.get("query_block").is_some());
    }
}

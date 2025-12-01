//! PostgreSQL backend tests

#[cfg(test)]
mod tests {
    use crate::explain::postgres::{build_postgres_explain_sql, ExplainFormat, ExplainOptions};

    #[test]
    fn test_explain_sql_basic() {
        let options = ExplainOptions {
            analyze: false,
            format: ExplainFormat::Text,
        };
        let sql = build_postgres_explain_sql("SELECT * FROM users", &options).unwrap();
        assert!(sql.contains("EXPLAIN"));
        assert!(sql.contains("SELECT * FROM users"));
    }

    #[test]
    fn test_explain_sql_analyze() {
        let options = ExplainOptions {
            analyze: true,
            format: ExplainFormat::Text,
        };
        let sql = build_postgres_explain_sql("SELECT * FROM users", &options).unwrap();
        assert!(sql.contains("ANALYZE"));
    }

    #[test]
    fn test_explain_sql_json() {
        let options = ExplainOptions {
            analyze: false,
            format: ExplainFormat::Json,
        };
        let sql = build_postgres_explain_sql("SELECT * FROM users", &options).unwrap();
        assert!(sql.contains("FORMAT JSON"));
    }
}

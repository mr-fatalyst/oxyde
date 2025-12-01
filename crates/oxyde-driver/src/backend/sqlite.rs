//! SQLite backend tests

#[cfg(test)]
mod tests {
    use crate::explain::postgres::{ExplainFormat, ExplainOptions};
    use crate::explain::sqlite::build_sqlite_explain_sql;

    #[test]
    fn test_explain_sql_basic() {
        let options = ExplainOptions {
            analyze: false,
            format: ExplainFormat::Text,
        };
        let sql = build_sqlite_explain_sql("SELECT * FROM users", &options).unwrap();
        assert!(sql.contains("EXPLAIN"));
        assert!(sql.contains("SELECT * FROM users"));
    }

    #[test]
    fn test_explain_sql_query_plan() {
        let options = ExplainOptions {
            analyze: false,
            format: ExplainFormat::Text,
        };
        let sql = build_sqlite_explain_sql("SELECT * FROM users", &options).unwrap();
        assert!(sql.contains("QUERY PLAN"));
    }
}

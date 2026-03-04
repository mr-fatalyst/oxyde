//! Unit tests for private helper functions.
//! Integration tests for public API are in tests/migration_tests.rs

use crate::sql::{python_type_to_sql, translate_db_type};
use crate::types::Dialect;

#[test]
fn test_python_type_to_sql_all_dialects() {
    // Test int type across dialects
    assert_eq!(python_type_to_sql("int", Dialect::Sqlite, false), "INTEGER");
    assert_eq!(
        python_type_to_sql("int", Dialect::Postgres, false),
        "BIGINT"
    );
    assert_eq!(python_type_to_sql("int", Dialect::Postgres, true), "SERIAL"); // PK
    assert_eq!(python_type_to_sql("int", Dialect::Mysql, false), "BIGINT");

    // Test bool type
    assert_eq!(
        python_type_to_sql("bool", Dialect::Sqlite, false),
        "INTEGER"
    );
    assert_eq!(
        python_type_to_sql("bool", Dialect::Postgres, false),
        "BOOLEAN"
    );
    assert_eq!(python_type_to_sql("bool", Dialect::Mysql, false), "TINYINT");

    // Test datetime type
    assert_eq!(
        python_type_to_sql("datetime", Dialect::Sqlite, false),
        "TEXT"
    );
    assert_eq!(
        python_type_to_sql("datetime", Dialect::Postgres, false),
        "TIMESTAMP"
    );
    assert_eq!(
        python_type_to_sql("datetime", Dialect::Mysql, false),
        "DATETIME"
    );

    // Test uuid type
    assert_eq!(python_type_to_sql("uuid", Dialect::Sqlite, false), "TEXT");
    assert_eq!(python_type_to_sql("uuid", Dialect::Postgres, false), "UUID");
    assert_eq!(
        python_type_to_sql("uuid", Dialect::Mysql, false),
        "CHAR(36)"
    );

    // Test bytes type
    assert_eq!(python_type_to_sql("bytes", Dialect::Sqlite, false), "BLOB");
    assert_eq!(
        python_type_to_sql("bytes", Dialect::Postgres, false),
        "BYTEA"
    );
    assert_eq!(python_type_to_sql("bytes", Dialect::Mysql, false), "BLOB");
}

#[test]
fn test_translate_db_type_serial() {
    // SERIAL is PostgreSQL-specific, should translate for other dialects
    assert_eq!(translate_db_type("SERIAL", Dialect::Postgres), "SERIAL");
    assert_eq!(translate_db_type("SERIAL", Dialect::Sqlite), "INTEGER");
    assert_eq!(translate_db_type("SERIAL", Dialect::Mysql), "INT");

    assert_eq!(
        translate_db_type("BIGSERIAL", Dialect::Postgres),
        "BIGSERIAL"
    );
    assert_eq!(translate_db_type("BIGSERIAL", Dialect::Sqlite), "INTEGER");
    assert_eq!(translate_db_type("BIGSERIAL", Dialect::Mysql), "BIGINT");
}

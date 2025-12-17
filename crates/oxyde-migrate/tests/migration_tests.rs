//! Integration tests for oxyde-migrate.
//!
//! Tests for public API: MigrationOp::to_sql(), compute_diff(), Snapshot.

use oxyde_migrate::{
    compute_diff, CheckDef, Dialect, FieldDef, ForeignKeyDef, IndexDef, MigrationOp, Snapshot,
    TableDef,
};

fn sample_field(name: &str) -> FieldDef {
    FieldDef {
        name: name.to_string(),
        python_type: "str".into(),
        db_type: None,
        nullable: false,
        primary_key: false,
        unique: false,
        default: None,
        auto_increment: false,
    }
}

fn sample_table() -> TableDef {
    TableDef {
        name: "users".into(),
        fields: vec![
            FieldDef {
                name: "id".into(),
                python_type: "int".into(),
                db_type: None,
                nullable: false,
                primary_key: true,
                unique: true,
                default: None,
                auto_increment: false,
            },
            sample_field("email"),
        ],
        indexes: vec![IndexDef {
            name: "users_email_idx".into(),
            fields: vec!["email".into()],
            unique: true,
            method: Some("btree".into()),
        }],
        foreign_keys: vec![],
        checks: vec![],
        comment: Some("User accounts".into()),
    }
}

#[test]
fn test_snapshot_serialization_roundtrip() {
    let mut snapshot = Snapshot::new();
    snapshot.add_table(sample_table());

    let json = snapshot.to_json().unwrap();
    let deserialized = Snapshot::from_json(&json).unwrap();
    assert_eq!(snapshot, deserialized);
}

#[test]
fn test_migration_create_table_generates_sql() {
    let sql = MigrationOp::CreateTable {
        table: sample_table(),
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert!(sql[0].contains("CREATE TABLE users"));
    assert!(sql[1].contains("CREATE UNIQUE INDEX users_email_idx"));
}

#[test]
fn test_sqlite_create_table_with_fk_inline() {
    // SQLite should have FK constraints inline in CREATE TABLE, not as ALTER TABLE
    let table = TableDef {
        name: "posts".into(),
        fields: vec![
            FieldDef {
                name: "id".into(),
                python_type: "int".into(),
                db_type: None,
                nullable: false,
                primary_key: true,
                unique: false,
                default: None,
                auto_increment: true,
            },
            FieldDef {
                name: "author_id".into(),
                python_type: "int".into(),
                db_type: None,
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
                auto_increment: false,
            },
        ],
        indexes: vec![],
        foreign_keys: vec![ForeignKeyDef {
            name: "fk_posts_author".into(),
            columns: vec!["author_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some("CASCADE".into()),
            on_update: None,
        }],
        checks: vec![CheckDef {
            name: "valid_author".into(),
            expression: "author_id > 0".into(),
        }],
        comment: None,
    };

    let sql = MigrationOp::CreateTable { table }
        .to_sql(Dialect::Sqlite)
        .unwrap();

    // Should have only 1 statement (CREATE TABLE with inline FK and CHECK)
    assert_eq!(
        sql.len(),
        1,
        "SQLite should not generate ALTER TABLE for FK"
    );

    let create_stmt = &sql[0];
    assert!(
        create_stmt.contains("FOREIGN KEY (author_id) REFERENCES users (id)"),
        "FK should be inline: {}",
        create_stmt
    );
    assert!(
        create_stmt.contains("ON DELETE CASCADE"),
        "ON DELETE should be present: {}",
        create_stmt
    );
    assert!(
        create_stmt.contains("CHECK (author_id > 0)"),
        "CHECK should be inline: {}",
        create_stmt
    );
    assert!(
        !create_stmt.contains("ALTER TABLE"),
        "Should not contain ALTER TABLE: {}",
        create_stmt
    );
}

#[test]
fn test_postgres_create_table_with_fk_as_alter() {
    // PostgreSQL should have FK constraints as separate ALTER TABLE
    let table = TableDef {
        name: "posts".into(),
        fields: vec![FieldDef {
            name: "id".into(),
            python_type: "int".into(),
            db_type: None,
            nullable: false,
            primary_key: true,
            unique: false,
            default: None,
            auto_increment: false,
        }],
        indexes: vec![],
        foreign_keys: vec![ForeignKeyDef {
            name: "fk_posts_author".into(),
            columns: vec!["author_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some("CASCADE".into()),
            on_update: None,
        }],
        checks: vec![],
        comment: None,
    };

    let sql = MigrationOp::CreateTable { table }
        .to_sql(Dialect::Postgres)
        .unwrap();

    // Should have 2 statements (CREATE TABLE + ALTER TABLE for FK)
    assert_eq!(
        sql.len(),
        2,
        "PostgreSQL should generate ALTER TABLE for FK"
    );
    assert!(sql[1].contains("ALTER TABLE posts ADD CONSTRAINT"));
    assert!(sql[1].contains("FOREIGN KEY"));
}

#[test]
fn test_sqlite_add_foreign_key_returns_error() {
    let fk = ForeignKeyDef {
        name: "fk_test".into(),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
    };

    let result = MigrationOp::AddForeignKey {
        table: "posts".into(),
        fk,
    }
    .to_sql(Dialect::Sqlite);

    assert!(result.is_err(), "SQLite AddForeignKey should return error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("SQLite does not support ALTER TABLE ADD FOREIGN KEY"),
        "Error message should mention limitation: {}",
        err
    );
}

#[test]
fn test_sqlite_add_check_returns_error() {
    let check = CheckDef {
        name: "valid_age".into(),
        expression: "age >= 0".into(),
    };

    let result = MigrationOp::AddCheck {
        table: "users".into(),
        check,
    }
    .to_sql(Dialect::Sqlite);

    assert!(result.is_err(), "SQLite AddCheck should return error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("SQLite does not support ALTER TABLE ADD CHECK"),
        "Error message should mention limitation: {}",
        err
    );
}

#[test]
fn test_migration_add_column_sql() {
    let sql = MigrationOp::AddColumn {
        table: "users".into(),
        field: sample_field("name"),
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert_eq!(
        sql,
        vec!["ALTER TABLE users ADD COLUMN name TEXT NOT NULL".to_string()]
    );
}

#[test]
fn test_dialect_specific_sql() {
    // Test SQLite AUTOINCREMENT
    let pk_field = FieldDef {
        name: "id".into(),
        python_type: "int".into(),
        db_type: None,
        nullable: false,
        primary_key: true,
        unique: false,
        default: None,
        auto_increment: true,
    };
    let table = TableDef {
        name: "test".into(),
        fields: vec![pk_field],
        indexes: vec![],
        foreign_keys: vec![],
        checks: vec![],
        comment: None,
    };
    let sql = MigrationOp::CreateTable {
        table: table.clone(),
    }
    .to_sql(Dialect::Sqlite)
    .unwrap();
    assert!(sql[0].contains("AUTOINCREMENT"));

    // Test MySQL AUTO_INCREMENT
    let sql = MigrationOp::CreateTable {
        table: table.clone(),
    }
    .to_sql(Dialect::Mysql)
    .unwrap();
    assert!(sql[0].contains("AUTO_INCREMENT"));

    // Test DROP INDEX MySQL vs others
    let dummy_index_def = IndexDef {
        name: "idx_name".into(),
        fields: vec!["name".into()],
        unique: false,
        method: None,
    };
    let drop_idx_mysql = MigrationOp::DropIndex {
        table: "users".into(),
        index: "idx_name".into(),
        index_def: dummy_index_def.clone(),
    }
    .to_sql(Dialect::Mysql)
    .unwrap();
    assert!(drop_idx_mysql[0].contains("ON users"));

    let drop_idx_pg = MigrationOp::DropIndex {
        table: "users".into(),
        index: "idx_name".into(),
        index_def: dummy_index_def,
    }
    .to_sql(Dialect::Postgres)
    .unwrap();
    assert!(!drop_idx_pg[0].contains("ON users"));
}

#[test]
fn test_compute_diff_detects_new_table_and_column() {
    let old = Snapshot::new();
    let mut new_snapshot = Snapshot::new();
    let mut table = sample_table();
    table.fields.push(sample_field("name"));
    new_snapshot.add_table(table);

    let ops = compute_diff(&old, &new_snapshot);
    assert!(matches!(ops[0], MigrationOp::CreateTable { .. }));
}

#[test]
fn test_sqlite_alter_column_returns_error_without_schema() {
    let old_field = FieldDef {
        name: "age".into(),
        python_type: "int".into(),
        db_type: None,
        nullable: true,
        primary_key: false,
        unique: false,
        default: None,
        auto_increment: false,
    };
    let new_field = FieldDef {
        name: "age".into(),
        python_type: "str".into(), // type change
        db_type: None,
        nullable: true,
        primary_key: false,
        unique: false,
        default: None,
        auto_increment: false,
    };

    let result = MigrationOp::AlterColumn {
        table: "users".into(),
        old_field,
        new_field,
        table_fields: None, // No schema - should error
        table_indexes: None,
        table_foreign_keys: None,
        table_checks: None,
    }
    .to_sql(Dialect::Sqlite);

    assert!(
        result.is_err(),
        "SQLite AlterColumn without schema should return error"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("SQLite does not support ALTER COLUMN"),
        "Error should mention SQLite limitation: {}",
        err
    );
}

#[test]
fn test_sqlite_alter_column_with_schema_generates_rebuild() {
    let old_field = FieldDef {
        name: "age".into(),
        python_type: "int".into(),
        db_type: None,
        nullable: true,
        primary_key: false,
        unique: false,
        default: None,
        auto_increment: false,
    };
    let new_field = FieldDef {
        name: "age".into(),
        python_type: "str".into(), // type change
        db_type: None,
        nullable: false, // nullable change
        primary_key: false,
        unique: false,
        default: None,
        auto_increment: false,
    };

    // Full table schema
    let table_fields = vec![
        FieldDef {
            name: "id".into(),
            python_type: "int".into(),
            db_type: None,
            nullable: false,
            primary_key: true,
            unique: false,
            default: None,
            auto_increment: true,
        },
        old_field.clone(),
        FieldDef {
            name: "name".into(),
            python_type: "str".into(),
            db_type: None,
            nullable: false,
            primary_key: false,
            unique: false,
            default: None,
            auto_increment: false,
        },
    ];

    let table_indexes = vec![IndexDef {
        name: "users_name_idx".into(),
        fields: vec!["name".into()],
        unique: false,
        method: None,
    }];

    let result = MigrationOp::AlterColumn {
        table: "users".into(),
        old_field,
        new_field,
        table_fields: Some(table_fields),
        table_indexes: Some(table_indexes),
        table_foreign_keys: None,
        table_checks: None,
    }
    .to_sql(Dialect::Sqlite);

    assert!(
        result.is_ok(),
        "SQLite AlterColumn with schema should succeed"
    );
    let stmts = result.unwrap();

    // Verify rebuild sequence
    assert!(
        stmts[0].contains("PRAGMA foreign_keys=OFF"),
        "Should disable FK: {}",
        stmts[0]
    );
    assert!(
        stmts[1].contains("CREATE TABLE _new_users"),
        "Should create temp table: {}",
        stmts[1]
    );
    assert!(
        stmts[1].contains("age TEXT NOT NULL"),
        "Should have altered column: {}",
        stmts[1]
    );
    assert!(
        stmts[2].contains("INSERT INTO _new_users"),
        "Should copy data: {}",
        stmts[2]
    );
    assert!(
        stmts[3].contains("DROP TABLE users"),
        "Should drop old table: {}",
        stmts[3]
    );
    assert!(
        stmts[4].contains("RENAME TO users"),
        "Should rename temp table: {}",
        stmts[4]
    );
    assert!(
        stmts[5].contains("CREATE INDEX users_name_idx"),
        "Should recreate index: {}",
        stmts[5]
    );
    assert!(
        stmts[6].contains("PRAGMA foreign_keys=ON"),
        "Should enable FK: {}",
        stmts[6]
    );
}

#[test]
fn test_rename_column_mysql_with_field_def() {
    let field_def = FieldDef {
        name: "old_name".into(),
        python_type: "str".into(),
        db_type: Some("VARCHAR(255)".into()),
        nullable: false,
        primary_key: false,
        unique: true,
        default: Some("'default'".into()),
        auto_increment: false,
    };

    let sql = MigrationOp::RenameColumn {
        table: "users".into(),
        old_name: "old_name".into(),
        new_name: "new_name".into(),
        field_def: Some(field_def),
    }
    .to_sql(Dialect::Mysql)
    .unwrap();

    assert_eq!(sql.len(), 1, "Should produce single SQL statement");
    let stmt = &sql[0];
    assert!(stmt.contains("CHANGE"), "Should use CHANGE: {}", stmt);
    assert!(
        stmt.contains("old_name"),
        "Should reference old name: {}",
        stmt
    );
    assert!(
        stmt.contains("new_name"),
        "Should contain new name: {}",
        stmt
    );
    assert!(
        stmt.contains("VARCHAR(255)"),
        "Should preserve type: {}",
        stmt
    );
    assert!(
        stmt.contains("NOT NULL"),
        "Should preserve NOT NULL: {}",
        stmt
    );
    assert!(stmt.contains("UNIQUE"), "Should preserve UNIQUE: {}", stmt);
    assert!(
        stmt.contains("DEFAULT"),
        "Should preserve DEFAULT: {}",
        stmt
    );
}

#[test]
fn test_rename_column_mysql_without_field_def_fallback() {
    let sql = MigrationOp::RenameColumn {
        table: "users".into(),
        old_name: "old_name".into(),
        new_name: "new_name".into(),
        field_def: None, // No field_def - should use fallback
    }
    .to_sql(Dialect::Mysql)
    .unwrap();

    assert_eq!(sql.len(), 2, "Should produce warning + SQL");
    assert!(
        sql[0].contains("WARNING"),
        "First line should be warning: {}",
        sql[0]
    );
    assert!(sql[1].contains("CHANGE"), "Should use CHANGE: {}", sql[1]);
    assert!(
        sql[1].contains("TEXT"),
        "Fallback should use TEXT: {}",
        sql[1]
    );
}

#[test]
fn test_compute_diff_detects_alter_column() {
    // Create old snapshot with a table
    let mut old = Snapshot::new();
    let old_table = TableDef {
        name: "users".into(),
        fields: vec![
            FieldDef {
                name: "id".into(),
                python_type: "int".into(),
                db_type: None,
                nullable: false,
                primary_key: true,
                unique: false,
                default: None,
                auto_increment: true,
            },
            FieldDef {
                name: "email".into(),
                python_type: "str".into(),
                db_type: Some("VARCHAR(100)".into()),
                nullable: false,
                primary_key: false,
                unique: true,
                default: None,
                auto_increment: false,
            },
        ],
        indexes: vec![],
        foreign_keys: vec![],
        checks: vec![],
        comment: None,
    };
    old.add_table(old_table);

    // Create new snapshot with modified email field
    let mut new_snapshot = Snapshot::new();
    let new_table = TableDef {
        name: "users".into(),
        fields: vec![
            FieldDef {
                name: "id".into(),
                python_type: "int".into(),
                db_type: None,
                nullable: false,
                primary_key: true,
                unique: false,
                default: None,
                auto_increment: true,
            },
            FieldDef {
                name: "email".into(),
                python_type: "str".into(),
                db_type: Some("VARCHAR(255)".into()), // Changed db_type
                nullable: true,                       // Changed nullable
                primary_key: false,
                unique: true,
                default: None,
                auto_increment: false,
            },
        ],
        indexes: vec![],
        foreign_keys: vec![],
        checks: vec![],
        comment: None,
    };
    new_snapshot.add_table(new_table);

    let ops = compute_diff(&old, &new_snapshot);

    // Should detect AlterColumn for email field
    assert_eq!(ops.len(), 1, "Should have exactly one operation");
    match &ops[0] {
        MigrationOp::AlterColumn {
            table,
            old_field,
            new_field,
            ..
        } => {
            assert_eq!(table, "users");
            assert_eq!(old_field.name, "email");
            assert_eq!(old_field.db_type, Some("VARCHAR(100)".into()));
            assert_eq!(new_field.db_type, Some("VARCHAR(255)".into()));
            assert!(!old_field.nullable);
            assert!(new_field.nullable);
        }
        other => panic!("Expected AlterColumn, got {:?}", other),
    }
}

#[test]
fn test_postgres_alter_column_unique_constraint() {
    // Test adding unique constraint
    let old_field = FieldDef {
        name: "email".into(),
        python_type: "str".into(),
        db_type: None,
        nullable: false,
        primary_key: false,
        unique: false,
        default: None,
        auto_increment: false,
    };
    let new_field = FieldDef {
        name: "email".into(),
        python_type: "str".into(),
        db_type: None,
        nullable: false,
        primary_key: false,
        unique: true, // Changed to unique
        default: None,
        auto_increment: false,
    };

    let sql = MigrationOp::AlterColumn {
        table: "users".into(),
        old_field: old_field.clone(),
        new_field: new_field.clone(),
        table_fields: None,
        table_indexes: None,
        table_foreign_keys: None,
        table_checks: None,
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert_eq!(sql.len(), 1, "Should have one statement");
    assert!(
        sql[0].contains("ADD CONSTRAINT"),
        "Should add constraint: {}",
        sql[0]
    );
    assert!(
        sql[0].contains("UNIQUE"),
        "Should be UNIQUE constraint: {}",
        sql[0]
    );

    // Test removing unique constraint
    let sql = MigrationOp::AlterColumn {
        table: "users".into(),
        old_field: new_field,
        new_field: old_field,
        table_fields: None,
        table_indexes: None,
        table_foreign_keys: None,
        table_checks: None,
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert_eq!(sql.len(), 1, "Should have one statement");
    assert!(
        sql[0].contains("DROP CONSTRAINT"),
        "Should drop constraint: {}",
        sql[0]
    );
}

#[test]
fn test_compute_diff_detects_dropped_table() {
    let mut old = Snapshot::new();
    old.add_table(sample_table());

    let new = Snapshot::new(); // Empty - table dropped

    let ops = compute_diff(&old, &new);
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        MigrationOp::DropTable { name, table } => {
            assert_eq!(name, "users");
            assert!(table.is_some(), "Should include table def for rollback");
        }
        _ => panic!("Expected DropTable"),
    }
}

#[test]
fn test_compute_diff_detects_dropped_column() {
    let mut old = Snapshot::new();
    old.add_table(sample_table());

    let mut new = Snapshot::new();
    let mut table = sample_table();
    table.fields.retain(|f| f.name != "email"); // Remove email column
    new.add_table(table);

    let ops = compute_diff(&old, &new);
    let drop_ops: Vec<_> = ops
        .iter()
        .filter(|op| matches!(op, MigrationOp::DropColumn { .. }))
        .collect();
    assert_eq!(drop_ops.len(), 1);
    match drop_ops[0] {
        MigrationOp::DropColumn {
            table,
            field,
            field_def,
        } => {
            assert_eq!(table, "users");
            assert_eq!(field, "email");
            assert!(field_def.is_some());
        }
        _ => panic!("Expected DropColumn"),
    }
}

#[test]
fn test_compute_diff_detects_index_changes() {
    let mut old = Snapshot::new();
    old.add_table(sample_table());

    let mut new = Snapshot::new();
    let mut table = sample_table();
    table.indexes.clear(); // Remove index
    table.indexes.push(IndexDef {
        name: "users_name_idx".into(), // New index
        fields: vec!["name".into()],
        unique: false,
        method: None,
    });
    new.add_table(table);

    let ops = compute_diff(&old, &new);

    let create_idx = ops.iter().any(
        |op| matches!(op, MigrationOp::CreateIndex { index, .. } if index.name == "users_name_idx"),
    );
    let drop_idx = ops
        .iter()
        .any(|op| matches!(op, MigrationOp::DropIndex { index, .. } if index == "users_email_idx"));

    assert!(create_idx, "Should detect new index");
    assert!(drop_idx, "Should detect dropped index");
}

#[test]
fn test_drop_table_sql() {
    let sql = MigrationOp::DropTable {
        name: "users".into(),
        table: None,
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert_eq!(sql.len(), 1);
    assert_eq!(sql[0], "DROP TABLE users");
}

#[test]
fn test_create_drop_index_sql() {
    // CreateIndex
    let sql = MigrationOp::CreateIndex {
        table: "users".into(),
        index: IndexDef {
            name: "users_email_idx".into(),
            fields: vec!["email".into()],
            unique: true,
            method: Some("btree".into()),
        },
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert_eq!(sql.len(), 1);
    assert!(sql[0].contains("CREATE UNIQUE INDEX users_email_idx"));
    assert!(sql[0].contains("USING btree"));

    // DropIndex
    let sql = MigrationOp::DropIndex {
        table: "users".into(),
        index: "users_email_idx".into(),
        index_def: IndexDef {
            name: "users_email_idx".into(),
            fields: vec!["email".into()],
            unique: true,
            method: None,
        },
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert_eq!(sql.len(), 1);
    assert_eq!(sql[0], "DROP INDEX users_email_idx");
}

#[test]
fn test_rename_table_sql() {
    let sql = MigrationOp::RenameTable {
        old_name: "users".into(),
        new_name: "accounts".into(),
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert_eq!(sql.len(), 1);
    assert!(sql[0].contains("RENAME") || sql[0].contains("ALTER TABLE"));
}

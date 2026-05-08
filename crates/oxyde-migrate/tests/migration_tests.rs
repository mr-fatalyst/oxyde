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
        max_length: None,
        max_digits: None,
        decimal_places: None,
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
                max_length: None,
                max_digits: None,
                decimal_places: None,
            },
            sample_field("email"),
        ],
        indexes: vec![IndexDef {
            name: "users_email_idx".into(),
            fields: vec!["email".into()],
            unique: true,
            method: Some("btree".into()),
            where_clause: None,
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

    assert!(sql[0].contains(r#"CREATE TABLE "users""#));
    assert!(sql[1].contains(r#"CREATE UNIQUE INDEX "users_email_idx""#));
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
                max_length: None,
                max_digits: None,
                decimal_places: None,
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
                max_length: None,
                max_digits: None,
                decimal_places: None,
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
        create_stmt.contains(r#"FOREIGN KEY ("author_id") REFERENCES "users" ("id")"#),
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
            max_length: None,
            max_digits: None,
            decimal_places: None,
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
    assert!(sql[1].contains("ADD CONSTRAINT"));
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
        vec![r#"ALTER TABLE "users" ADD COLUMN "name" VARCHAR(255) NOT NULL"#.to_string()]
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
        max_length: None,
        max_digits: None,
        decimal_places: None,
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
        where_clause: None,
    };
    let drop_idx_mysql = MigrationOp::DropIndex {
        table: "users".into(),
        name: "idx_name".into(),
        index_def: Some(dummy_index_def.clone()),
    }
    .to_sql(Dialect::Mysql)
    .unwrap();
    assert!(drop_idx_mysql[0].contains("ON"));

    let drop_idx_pg = MigrationOp::DropIndex {
        table: "users".into(),
        name: "idx_name".into(),
        index_def: Some(dummy_index_def),
    }
    .to_sql(Dialect::Postgres)
    .unwrap();
    // PostgreSQL DROP INDEX doesn't include the table name
    assert!(!drop_idx_pg[0].contains("ON "));
}

#[test]
fn test_compute_diff_detects_new_table_and_column() {
    let old = Snapshot::new();
    let mut new_snapshot = Snapshot::new();
    let mut table = sample_table();
    table.fields.push(sample_field("name"));
    new_snapshot.add_table(table);

    let ops = compute_diff(&old, &new_snapshot).unwrap();
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
        max_length: None,
        max_digits: None,
        decimal_places: None,
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
        max_length: None,
        max_digits: None,
        decimal_places: None,
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
        max_length: None,
        max_digits: None,
        decimal_places: None,
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
        max_length: None,
        max_digits: None,
        decimal_places: None,
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
            max_length: None,
            max_digits: None,
            decimal_places: None,
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
            max_length: None,
            max_digits: None,
            decimal_places: None,
        },
    ];

    let table_indexes = vec![IndexDef {
        name: "users_name_idx".into(),
        fields: vec!["name".into()],
        unique: false,
        method: None,
        where_clause: None,
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
        stmts[1].contains("_new_users"),
        "Should create temp table: {}",
        stmts[1]
    );
    assert!(
        stmts[1].contains("VARCHAR(255) NOT NULL"),
        "Should have altered column: {}",
        stmts[1]
    );
    assert!(
        stmts[2].contains("_new_users"),
        "Should copy data: {}",
        stmts[2]
    );
    assert!(
        stmts[3].contains("DROP TABLE"),
        "Should drop old table: {}",
        stmts[3]
    );
    assert!(
        stmts[4].contains("RENAME"),
        "Should rename temp table: {}",
        stmts[4]
    );
    assert!(
        stmts[5].contains("users_name_idx"),
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
        max_length: None,
        max_digits: None,
        decimal_places: None,
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
                max_length: None,
                max_digits: None,
                decimal_places: None,
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
                max_length: None,
                max_digits: None,
                decimal_places: None,
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
                max_length: None,
                max_digits: None,
                decimal_places: None,
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
                max_length: None,
                max_digits: None,
                decimal_places: None,
            },
        ],
        indexes: vec![],
        foreign_keys: vec![],
        checks: vec![],
        comment: None,
    };
    new_snapshot.add_table(new_table);

    let ops = compute_diff(&old, &new_snapshot).unwrap();

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
        max_length: None,
        max_digits: None,
        decimal_places: None,
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
        max_length: None,
        max_digits: None,
        decimal_places: None,
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

    let ops = compute_diff(&old, &new).unwrap();
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

    let ops = compute_diff(&old, &new).unwrap();
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
        where_clause: None,
    });
    new.add_table(table);

    let ops = compute_diff(&old, &new).unwrap();

    let create_idx = ops.iter().any(
        |op| matches!(op, MigrationOp::CreateIndex { index, .. } if index.name == "users_name_idx"),
    );
    let drop_idx = ops
        .iter()
        .any(|op| matches!(op, MigrationOp::DropIndex { name, .. } if name == "users_email_idx"));

    assert!(create_idx, "Should detect new index");
    assert!(drop_idx, "Should detect dropped index");
}

#[test]
fn test_compute_diff_detects_partial_index_predicate_change() {
    let mut old = Snapshot::new();
    old.add_table(sample_table());

    let mut new = Snapshot::new();
    let mut table = sample_table();
    table.indexes[0].where_clause = Some("deleted_at IS NULL".into());
    new.add_table(table);

    let ops = compute_diff(&old, &new).unwrap();

    assert!(
        matches!(ops.first(), Some(MigrationOp::DropIndex { name, .. }) if name == "users_email_idx")
    );
    assert!(
        matches!(
            ops.get(1),
            Some(MigrationOp::CreateIndex { index, .. })
                if index.name == "users_email_idx"
                    && index.where_clause.as_deref() == Some("deleted_at IS NULL")
        ),
        "predicate changes should rebuild the index, got {:?}",
        ops
    );
}

#[test]
fn test_compute_diff_ignores_partial_index_predicate_whitespace() {
    let mut old = Snapshot::new();
    old.add_table(sample_table());

    let mut new = Snapshot::new();
    let mut table = sample_table();
    table.indexes[0].where_clause = Some("  deleted_at IS NULL  ".into());
    new.add_table(table);

    let mut old_table = old.tables.get_mut("users").unwrap().clone();
    old_table.indexes[0].where_clause = Some("deleted_at IS NULL".into());
    old.tables.insert("users".into(), old_table);

    let ops = compute_diff(&old, &new).unwrap();

    assert!(
        ops.is_empty(),
        "whitespace-only predicate changes should not diff"
    );
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
    assert_eq!(sql[0], r#"DROP TABLE "users""#);
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
            where_clause: None,
        },
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert_eq!(sql.len(), 1);
    assert!(sql[0].contains("CREATE UNIQUE INDEX"));
    assert!(sql[0].contains("users_email_idx"));
    assert!(sql[0].contains("btree"));

    // DropIndex
    let sql = MigrationOp::DropIndex {
        table: "users".into(),
        name: "users_email_idx".into(),
        index_def: Some(IndexDef {
            name: "users_email_idx".into(),
            fields: vec!["email".into()],
            unique: true,
            method: None,
            where_clause: None,
        }),
    }
    .to_sql(Dialect::Postgres)
    .unwrap();

    assert_eq!(sql.len(), 1);
    assert!(sql[0].contains("DROP INDEX"));
    assert!(sql[0].contains("users_email_idx"));
}

#[test]
fn test_partial_index_sql() {
    let index = IndexDef {
        name: "users_active_email_idx".into(),
        fields: vec!["email".into()],
        unique: true,
        method: Some("btree".into()),
        where_clause: Some("deleted_at IS NULL".into()),
    };

    for dialect in [Dialect::Postgres, Dialect::Sqlite] {
        let sql = MigrationOp::CreateIndex {
            table: "users".into(),
            index: index.clone(),
        }
        .to_sql(dialect)
        .unwrap();

        assert_eq!(sql.len(), 1);
        assert!(sql[0].contains("WHERE deleted_at IS NULL"));
    }

    let err = MigrationOp::CreateIndex {
        table: "users".into(),
        index,
    }
    .to_sql(Dialect::Mysql)
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("MySQL does not support partial indexes"));
}

#[test]
fn test_partial_index_json_roundtrip_trims_predicate() {
    let snapshot = Snapshot::from_json(
        r#"{
            "version": 1,
            "tables": {
                "users": {
                    "name": "users",
                    "fields": [],
                    "indexes": [{
                        "name": "users_active_email_idx",
                        "fields": ["email"],
                        "unique": true,
                        "method": "btree",
                        "where": "  deleted_at IS NULL  "
                    }],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": null
                }
            }
        }"#,
    )
    .unwrap();

    let json = snapshot.to_json().unwrap();

    assert!(json.contains(r#""where": "deleted_at IS NULL""#));
    assert!(!json.contains("  deleted_at IS NULL  "));
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

#[test]
fn test_array_field_preserves_constraints() {
    let table = TableDef {
        name: "items".into(),
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
                max_length: None,
                max_digits: None,
                decimal_places: None,
            },
            FieldDef {
                name: "tags".into(),
                python_type: "str[]".into(),
                db_type: None,
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
                auto_increment: false,
                max_length: Some(100),
                max_digits: None,
                decimal_places: None,
            },
            FieldDef {
                name: "prices".into(),
                python_type: "decimal[]".into(),
                db_type: None,
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
                auto_increment: false,
                max_length: None,
                max_digits: Some(10),
                decimal_places: Some(2),
            },
        ],
        indexes: vec![],
        foreign_keys: vec![],
        checks: vec![],
        comment: None,
    };

    // PostgreSQL: array with inner constraints
    let pg_sql = MigrationOp::CreateTable {
        table: table.clone(),
    }
    .to_sql(Dialect::Postgres)
    .unwrap();
    let create = &pg_sql[0];
    assert!(
        create.contains("VARCHAR(100)[]"),
        "PG str[] with max_length=100 should be VARCHAR(100)[], got: {create}"
    );
    assert!(
        create.contains("NUMERIC(10,2)[]"),
        "PG decimal[] with max_digits should be NUMERIC(10,2)[], got: {create}"
    );

    // MySQL: arrays become JSON
    let mysql_sql = MigrationOp::CreateTable {
        table: table.clone(),
    }
    .to_sql(Dialect::Mysql)
    .unwrap();
    let create = &mysql_sql[0];
    assert!(
        create.contains("JSON"),
        "MySQL arrays should be JSON, got: {create}"
    );

    // SQLite: arrays become TEXT
    let sqlite_sql = MigrationOp::CreateTable { table }
        .to_sql(Dialect::Sqlite)
        .unwrap();
    let create = &sqlite_sql[0];
    assert!(
        create.contains("TEXT"),
        "SQLite arrays should be TEXT, got: {create}"
    );
}

#[test]
fn test_compute_diff_emits_create_tables_in_topological_order() {
    // Chain: posts → comments → reactions (reactions depends on comments, etc.)
    // Topological order must be: posts, comments, reactions — independently
    // of HashMap iteration order.
    fn table_with_fk(name: &str, ref_table: Option<&str>) -> TableDef {
        let mut fks = Vec::new();
        if let Some(r) = ref_table {
            fks.push(ForeignKeyDef {
                name: format!("fk_{}_{}", name, r),
                columns: vec![format!("{}_id", r)],
                ref_table: r.to_string(),
                ref_columns: vec!["id".to_string()],
                on_delete: Some("CASCADE".to_string()),
                on_update: Some("NO ACTION".to_string()),
            });
        }
        TableDef {
            name: name.to_string(),
            fields: vec![FieldDef {
                name: "id".into(),
                python_type: "int".into(),
                db_type: None,
                nullable: false,
                primary_key: true,
                unique: true,
                default: None,
                auto_increment: false,
                max_length: None,
                max_digits: None,
                decimal_places: None,
            }],
            indexes: vec![],
            foreign_keys: fks,
            checks: vec![],
            comment: None,
        }
    }

    // Run multiple times: HashMap iteration order varies per process, but
    // topo-sort output must remain stable within a process — and across runs
    // after the fix.
    for _ in 0..10 {
        let old = Snapshot::new();
        let mut new = Snapshot::new();
        new.add_table(table_with_fk("reactions", Some("comments")));
        new.add_table(table_with_fk("comments", Some("posts")));
        new.add_table(table_with_fk("posts", None));

        let ops = compute_diff(&old, &new).unwrap();
        let create_names: Vec<&str> = ops
            .iter()
            .filter_map(|op| match op {
                MigrationOp::CreateTable { table } => Some(table.name.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(
            create_names,
            vec!["posts", "comments", "reactions"],
            "CreateTable ops must be in topological order (referenced tables first)"
        );
    }
}

#[test]
fn test_compute_diff_rejects_cyclic_foreign_keys() {
    // a → b → a cycle
    fn table_with_fk(name: &str, ref_table: &str) -> TableDef {
        TableDef {
            name: name.to_string(),
            fields: vec![FieldDef {
                name: "id".into(),
                python_type: "int".into(),
                db_type: None,
                nullable: false,
                primary_key: true,
                unique: true,
                default: None,
                auto_increment: false,
                max_length: None,
                max_digits: None,
                decimal_places: None,
            }],
            indexes: vec![],
            foreign_keys: vec![ForeignKeyDef {
                name: format!("fk_{}_{}", name, ref_table),
                columns: vec![format!("{}_id", ref_table)],
                ref_table: ref_table.to_string(),
                ref_columns: vec!["id".into()],
                on_delete: Some("NO ACTION".into()),
                on_update: Some("NO ACTION".into()),
            }],
            checks: vec![],
            comment: None,
        }
    }

    let old = Snapshot::new();
    let mut new = Snapshot::new();
    new.add_table(table_with_fk("a", "b"));
    new.add_table(table_with_fk("b", "a"));

    let result = compute_diff(&old, &new);
    assert!(result.is_err(), "cyclic FK schema must error out");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("cyclic foreign key"),
        "error should mention cycle: {err}"
    );
    assert!(err.contains("a") && err.contains("b"));
}

#[test]
fn test_compute_diff_emits_drop_tables_in_reverse_topological_order() {
    fn table_with_fk(name: &str, ref_table: Option<&str>) -> TableDef {
        let mut fks = Vec::new();
        if let Some(r) = ref_table {
            fks.push(ForeignKeyDef {
                name: format!("fk_{}_{}", name, r),
                columns: vec![format!("{}_id", r)],
                ref_table: r.to_string(),
                ref_columns: vec!["id".to_string()],
                on_delete: Some("CASCADE".to_string()),
                on_update: Some("NO ACTION".to_string()),
            });
        }
        TableDef {
            name: name.to_string(),
            fields: vec![FieldDef {
                name: "id".into(),
                python_type: "int".into(),
                db_type: None,
                nullable: false,
                primary_key: true,
                unique: true,
                default: None,
                auto_increment: false,
                max_length: None,
                max_digits: None,
                decimal_places: None,
            }],
            indexes: vec![],
            foreign_keys: fks,
            checks: vec![],
            comment: None,
        }
    }

    let mut old = Snapshot::new();
    old.add_table(table_with_fk("posts", None));
    old.add_table(table_with_fk("comments", Some("posts")));
    let new = Snapshot::new();

    let ops = compute_diff(&old, &new).unwrap();
    let drop_names: Vec<&str> = ops
        .iter()
        .filter_map(|op| match op {
            MigrationOp::DropTable { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(
        drop_names,
        vec!["comments", "posts"],
        "DropTable ops must be in reverse topological order (referencing tables first)"
    );
}

// Helper: build a table with a primary key and an arbitrary list of FK refs.
// Used by the cyclic-schema tests below.
fn cyclic_table(name: &str, fk_refs: &[(&str, &str)]) -> TableDef {
    let mut fields = vec![FieldDef {
        name: "id".into(),
        python_type: "int".into(),
        db_type: None,
        nullable: false,
        primary_key: true,
        unique: true,
        default: None,
        auto_increment: false,
        max_length: None,
        max_digits: None,
        decimal_places: None,
    }];
    let mut foreign_keys = Vec::new();
    for (col, ref_table) in fk_refs {
        fields.push(FieldDef {
            name: (*col).to_string(),
            python_type: "int".into(),
            db_type: None,
            nullable: true,
            primary_key: false,
            unique: false,
            default: None,
            auto_increment: false,
            max_length: None,
            max_digits: None,
            decimal_places: None,
        });
        foreign_keys.push(ForeignKeyDef {
            name: format!("fk_{}_{}", name, col),
            columns: vec![(*col).to_string()],
            ref_table: (*ref_table).to_string(),
            ref_columns: vec!["id".into()],
            on_delete: Some("NO ACTION".into()),
            on_update: Some("NO ACTION".into()),
        });
    }
    TableDef {
        name: name.to_string(),
        fields,
        indexes: vec![],
        foreign_keys,
        checks: vec![],
        comment: None,
    }
}

#[test]
fn test_compute_diff_noop_on_cyclic_schema() {
    // when old == new and the schema contains a
    // mutual-FK cycle, compute_diff must return [] instead of erroring out
    // on the topological sort. Cycle is only a problem when tables actually
    // need to be created/dropped — for unchanged tables it's irrelevant.
    let mut snap = Snapshot::new();
    snap.add_table(cyclic_table("companies", &[("user_id", "users")]));
    snap.add_table(cyclic_table("users", &[("company_id", "companies")]));

    let ops =
        compute_diff(&snap, &snap).expect("no-op diff on cyclic schema must succeed, not error");
    assert!(
        ops.is_empty(),
        "no-op diff must produce zero operations, got {:?}",
        ops
    );
}

#[test]
fn test_compute_diff_modify_column_in_cyclic_schema() {
    // old and new both contain the same mutual-FK cycle; new adds an extra
    // column to one of the cyclic tables. Diff must produce just the
    // AddColumn operation, not an error.
    let mut old = Snapshot::new();
    old.add_table(cyclic_table("companies", &[("user_id", "users")]));
    old.add_table(cyclic_table("users", &[("company_id", "companies")]));

    let mut new = Snapshot::new();
    let mut companies_v2 = cyclic_table("companies", &[("user_id", "users")]);
    companies_v2.fields.push(sample_field("name"));
    new.add_table(companies_v2);
    new.add_table(cyclic_table("users", &[("company_id", "companies")]));

    let ops = compute_diff(&old, &new).expect("modification on cyclic schema must succeed");

    let add_columns: Vec<&str> = ops
        .iter()
        .filter_map(|op| match op {
            MigrationOp::AddColumn { table, field } if table == "companies" => {
                Some(field.name.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        add_columns,
        vec!["name"],
        "expected exactly one AddColumn(companies.name), got ops={:?}",
        ops
    );
}

#[test]
fn test_compute_diff_create_table_referencing_cyclic_subset() {
    // old already contains a cyclic pair (companies <-> users). new adds a
    // third table (orders) that references users via FK. The CREATE subset
    // is just {orders} and is acyclic, so diff must succeed and emit a
    // CreateTable for orders. FKs from orders to the existing 'users' must
    // be ignored by the topo-sort because users is not in the create subset.
    let mut old = Snapshot::new();
    old.add_table(cyclic_table("companies", &[("user_id", "users")]));
    old.add_table(cyclic_table("users", &[("company_id", "companies")]));

    let mut new = Snapshot::new();
    new.add_table(cyclic_table("companies", &[("user_id", "users")]));
    new.add_table(cyclic_table("users", &[("company_id", "companies")]));
    new.add_table(cyclic_table("orders", &[("user_id", "users")]));

    let ops = compute_diff(&old, &new)
        .expect("creating an acyclic table on top of a cyclic schema must succeed");

    let create_names: Vec<&str> = ops
        .iter()
        .filter_map(|op| match op {
            MigrationOp::CreateTable { table } => Some(table.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        create_names,
        vec!["orders"],
        "expected exactly one CreateTable(orders), got ops={:?}",
        ops
    );
}

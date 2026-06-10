//! Golden DDL tests: byte-exact snapshots of generated SQL.
//!
//! # Purpose
//!
//! These tests freeze the SQL output of `Migration::to_sql()` (the production
//! entry point used by Python via `migration_to_sql`) for all dialects.
//! They exist to make any change in generated DDL **visible and intentional**
//! during refactorings of the type-mapping layer.
//!
//! # Rules
//!
//! 1. These snapshots capture the output **as is**, not "as it should be".
//!    Oddities found while recording are documented in the PR description,
//!    not fixed here.
//! 2. In future PRs, any snapshot diff must be a separate, consciously
//!    reviewed commit ("accept DDL changes") with a justification — or the
//!    mapping must be adjusted (e.g. `ColumnType::custom`) so the output
//!    stays byte-identical.
//!
//! # Workflow
//!
//! ```bash
//! cargo test -p oxyde-migrate --test golden_ddl     # compare against snapshots
//! cargo insta review                                 # review & accept pending
//! # or non-interactively: INSTA_UPDATE=always cargo test -p oxyde-migrate --test golden_ddl
//! ```

use oxyde_migrate::{
    CheckDef, Dialect, FieldDef, ForeignKeyDef, IndexDef, Migration, MigrationOp, TableDef,
};

const ALL_DIALECTS: &[Dialect] = &[Dialect::Postgres, Dialect::Mysql, Dialect::Sqlite];
const PG_MYSQL: &[Dialect] = &[Dialect::Postgres, Dialect::Mysql];
const PG_SQLITE: &[Dialect] = &[Dialect::Postgres, Dialect::Sqlite];

fn dialect_name(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::Postgres => "postgres",
        Dialect::Mysql => "mysql",
        Dialect::Sqlite => "sqlite",
    }
}

// ── Fixture builders ───────────────────────────────────────────────────────

fn field(name: &str, python_type: &str) -> FieldDef {
    FieldDef {
        name: name.into(),
        python_type: python_type.into(),
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

trait FieldDefExt: Sized {
    fn nullable(self) -> Self;
    fn unique(self) -> Self;
    fn pk(self) -> Self;
    fn auto_inc(self) -> Self;
    fn db_type(self, t: &str) -> Self;
    fn default_sql(self, d: &str) -> Self;
    fn max_len(self, n: u32) -> Self;
    fn decimal(self, digits: u32, places: u32) -> Self;
}

impl FieldDefExt for FieldDef {
    fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
    fn unique(mut self) -> Self {
        self.unique = true;
        self
    }
    fn pk(mut self) -> Self {
        self.primary_key = true;
        self
    }
    fn auto_inc(mut self) -> Self {
        self.auto_increment = true;
        self
    }
    fn db_type(mut self, t: &str) -> Self {
        self.db_type = Some(t.into());
        self
    }
    fn default_sql(mut self, d: &str) -> Self {
        self.default = Some(d.into());
        self
    }
    fn max_len(mut self, n: u32) -> Self {
        self.max_length = Some(n);
        self
    }
    fn decimal(mut self, digits: u32, places: u32) -> Self {
        self.max_digits = Some(digits);
        self.decimal_places = Some(places);
        self
    }
}

fn table(name: &str, fields: Vec<FieldDef>) -> TableDef {
    TableDef {
        name: name.into(),
        fields,
        indexes: vec![],
        foreign_keys: vec![],
        checks: vec![],
        comment: None,
    }
}

fn index(name: &str, fields: &[&str]) -> IndexDef {
    IndexDef {
        name: name.into(),
        fields: fields.iter().map(|f| (*f).into()).collect(),
        unique: false,
        method: None,
        where_clause: None,
    }
}

fn fk(
    name: &str,
    columns: &[&str],
    ref_table: &str,
    ref_columns: &[&str],
    on_delete: Option<&str>,
    on_update: Option<&str>,
) -> ForeignKeyDef {
    ForeignKeyDef {
        name: name.into(),
        columns: columns.iter().map(|c| (*c).into()).collect(),
        ref_table: ref_table.into(),
        ref_columns: ref_columns.iter().map(|c| (*c).into()).collect(),
        on_delete: on_delete.map(Into::into),
        on_update: on_update.map(Into::into),
    }
}

fn check(name: &str, expression: &str) -> CheckDef {
    CheckDef {
        name: name.into(),
        expression: expression.into(),
    }
}

// ── Rendering + snapshot helpers ───────────────────────────────────────────

/// Render ops through the production entry point (`Migration::to_sql`),
/// which also applies the "ALTER statements last" ordering.
fn render(ops: &[MigrationOp], dialect: Dialect) -> String {
    let migration = Migration {
        name: "golden".into(),
        operations: ops.to_vec(),
    };
    let stmts = migration
        .to_sql(dialect)
        .unwrap_or_else(|e| panic!("SQL generation failed for {dialect:?}: {e}"));
    stmts.join(";\n\n")
}

fn snap(name: &str, ops: &[MigrationOp], dialects: &[Dialect]) {
    for &dialect in dialects {
        let sql = render(ops, dialect);
        insta::assert_snapshot!(format!("{name}__{}", dialect_name(dialect)), sql);
    }
}

// ── 1. CreateTable: all python_type branches + constraints ────────────────

#[test]
fn create_table_kitchen_sink() {
    let ops = [MigrationOp::CreateTable {
        table: table(
            "kitchen_sink",
            vec![
                // Scalars — one column per python_type branch
                field("col_int", "int"),
                field("col_str", "str"),
                field("col_str_100", "str").max_len(100),
                field("col_float", "float"),
                field("col_bool", "bool"),
                field("col_bytes", "bytes"),
                field("col_datetime", "datetime"),
                field("col_date", "date"),
                field("col_time", "time"),
                field("col_timedelta", "timedelta"),
                field("col_uuid", "uuid"),
                field("col_decimal", "decimal"),
                field("col_decimal_10_2", "decimal").decimal(10, 2),
                field("col_json", "json"),
                field("col_unknown", "custom_thing"),
                // Arrays
                field("arr_int", "int[]"),
                field("arr_str", "str[]"),
                field("arr_uuid", "uuid[]"),
                field("arr_decimal", "decimal[]"),
                // Nullability / uniqueness
                field("col_nullable", "str").nullable(),
                field("col_unique", "str").unique(),
                // Defaults (raw SQL fragments)
                field("def_str", "str").default_sql("'hello'"),
                field("def_int", "int").default_sql("42"),
                field("def_float", "float").default_sql("1.5"),
                field("def_bool", "bool").default_sql("TRUE"),
                field("def_now", "datetime").default_sql("CURRENT_TIMESTAMP"),
            ],
        ),
    }];
    snap("create_table_kitchen_sink", &ops, ALL_DIALECTS);
}

// ── 2. CreateTable: primary key variants ───────────────────────────────────

#[test]
fn create_table_pk_variants() {
    let ops = [
        MigrationOp::CreateTable {
            table: table(
                "t_pk_int_auto",
                vec![field("id", "int").pk().auto_inc(), field("name", "str")],
            ),
        },
        MigrationOp::CreateTable {
            table: table(
                "t_pk_int_plain",
                vec![field("id", "int").pk(), field("name", "str")],
            ),
        },
        MigrationOp::CreateTable {
            table: table(
                "t_pk_uuid",
                vec![field("id", "uuid").pk(), field("name", "str")],
            ),
        },
        MigrationOp::CreateTable {
            table: table(
                "t_pk_str",
                vec![field("code", "str").pk().max_len(32), field("name", "str")],
            ),
        },
    ];
    snap("create_table_pk_variants", &ops, ALL_DIALECTS);
}

// ── 3. CreateTable: explicit db_type overrides ─────────────────────────────

#[test]
fn create_table_db_type_overrides() {
    let ops = [MigrationOp::CreateTable {
        table: table(
            "db_type_overrides",
            vec![
                field("c_serial", "int").db_type("SERIAL"),
                field("c_bigserial", "int").db_type("BIGSERIAL"),
                field("c_jsonb", "json").db_type("JSONB"),
                field("c_timestamptz", "datetime").db_type("TIMESTAMPTZ"),
                field("c_numeric", "decimal").db_type("NUMERIC(10,2)"),
                field("c_varchar", "str").db_type("VARCHAR(100)"),
                field("c_char36", "uuid").db_type("CHAR(36)"),
                field("c_money", "float").db_type("MONEY"),
            ],
        ),
    }];
    snap("create_table_db_type_overrides", &ops, ALL_DIALECTS);
}

// ── 4. CreateTable with FK / CHECK / index (+ ALTER-last ordering) ─────────

#[test]
fn create_table_with_fk() {
    let mut post = table(
        "post",
        vec![
            field("id", "int").pk().auto_inc(),
            field("title", "str"),
            field("author_id", "int"),
            field("editor_id", "int").nullable(),
            field("reviewer_id", "int").nullable(),
            field("owner_id", "int").nullable(),
        ],
    );
    post.indexes = vec![index("ix_post_title", &["title"])];
    post.foreign_keys = vec![
        fk(
            "fk_post_author",
            &["author_id"],
            "author",
            &["id"],
            Some("CASCADE"),
            Some("CASCADE"),
        ),
        fk(
            "fk_post_editor",
            &["editor_id"],
            "author",
            &["id"],
            Some("SET NULL"),
            Some("NO ACTION"),
        ),
        fk(
            "fk_post_reviewer",
            &["reviewer_id"],
            "author",
            &["id"],
            Some("RESTRICT"),
            Some("SET DEFAULT"),
        ),
        fk(
            "fk_post_owner",
            &["owner_id"],
            "author",
            &["id"],
            None,
            None,
        ),
    ];
    post.checks = vec![check("chk_post_title_len", "LENGTH(title) > 0")];

    let ops = [MigrationOp::CreateTable { table: post }];
    snap("create_table_with_fk", &ops, ALL_DIALECTS);
}

// ── 5. AddColumn / DropColumn / DropTable ───────────────────────────────────

#[test]
fn add_drop_column_table() {
    let ops = [
        MigrationOp::AddColumn {
            table: "users".into(),
            field: field("nickname", "str").nullable(),
        },
        MigrationOp::AddColumn {
            table: "users".into(),
            field: field("score", "int").default_sql("0"),
        },
        MigrationOp::DropColumn {
            table: "users".into(),
            field: "legacy_flag".into(),
            field_def: None,
        },
        MigrationOp::DropTable {
            name: "obsolete".into(),
            table: None,
        },
    ];
    snap("add_drop_column_table", &ops, ALL_DIALECTS);
}

// ── 6. AlterColumn (PG: per-change statements; MySQL: MODIFY COLUMN) ───────

#[test]
fn alter_column() {
    let ops = [
        // type change: int → str(50)
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("age", "int"),
            new_field: field("age", "str").max_len(50),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // NOT NULL → NULL
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("bio", "str"),
            new_field: field("bio", "str").nullable(),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // set default
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("status", "str"),
            new_field: field("status", "str").default_sql("'active'"),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // drop default
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("rank", "int").default_sql("0"),
            new_field: field("rank", "int"),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // add unique
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("email", "str"),
            new_field: field("email", "str").unique(),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // drop unique
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("phone", "str").unique(),
            new_field: field("phone", "str"),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
    ];
    snap("alter_column", &ops, PG_MYSQL);
}

// ── 7. AlterColumn on SQLite: full table rebuild ────────────────────────────

#[test]
fn alter_column_sqlite_rebuild() {
    let ops = [MigrationOp::AlterColumn {
        table: "users".into(),
        old_field: field("age", "int"),
        new_field: field("age", "str").max_len(50),
        table_fields: Some(vec![
            field("id", "int").pk().auto_inc(),
            field("name", "str"),
            field("age", "int"),
        ]),
        table_indexes: Some(vec![index("ix_users_name", &["name"])]),
        table_foreign_keys: Some(vec![fk(
            "fk_users_org",
            &["org_id"],
            "org",
            &["id"],
            Some("CASCADE"),
            None,
        )]),
        table_checks: Some(vec![check("chk_users_age", "age >= 0")]),
    }];
    snap("alter_column_sqlite_rebuild", &ops, &[Dialect::Sqlite]);
}

// ── 8. Index operations ─────────────────────────────────────────────────────

#[test]
fn index_ops() {
    let unique_composite = IndexDef {
        name: "ux_users_email_tenant".into(),
        fields: vec!["email".into(), "tenant_id".into()],
        unique: true,
        method: None,
        where_clause: None,
    };
    let gin = IndexDef {
        name: "ix_users_tags_gin".into(),
        fields: vec!["tags".into()],
        unique: false,
        method: Some("gin".into()),
        where_clause: None,
    };
    let hash = IndexDef {
        name: "ix_users_email_hash".into(),
        fields: vec!["email".into()],
        unique: false,
        method: Some("hash".into()),
        where_clause: None,
    };

    let ops = [
        MigrationOp::CreateIndex {
            table: "users".into(),
            index: index("ix_users_name", &["name"]),
        },
        MigrationOp::CreateIndex {
            table: "users".into(),
            index: unique_composite,
        },
        MigrationOp::CreateIndex {
            table: "users".into(),
            index: gin,
        },
        MigrationOp::CreateIndex {
            table: "users".into(),
            index: hash,
        },
        MigrationOp::DropIndex {
            table: "users".into(),
            name: "ix_users_old".into(),
            index_def: None,
        },
    ];
    snap("index_ops", &ops, ALL_DIALECTS);
}

/// Partial indexes are unsupported on MySQL (returns Err) — PG/SQLite only.
#[test]
fn index_partial() {
    let partial = IndexDef {
        name: "ix_users_active_email".into(),
        fields: vec!["email".into()],
        unique: true,
        method: None,
        where_clause: Some("deleted_at IS NULL".into()),
    };
    let ops = [MigrationOp::CreateIndex {
        table: "users".into(),
        index: partial,
    }];
    snap("index_partial", &ops, PG_SQLITE);
}

// ── 9. Foreign key operations (SQLite returns Err — excluded) ──────────────

#[test]
fn foreign_key_ops() {
    let ops = [
        MigrationOp::AddForeignKey {
            table: "post".into(),
            fk: fk(
                "fk_post_author",
                &["author_id"],
                "author",
                &["id"],
                Some("CASCADE"),
                Some("NO ACTION"),
            ),
        },
        MigrationOp::DropForeignKey {
            table: "post".into(),
            name: "fk_post_editor".into(),
            fk_def: None,
        },
    ];
    snap("foreign_key_ops", &ops, PG_MYSQL);
}

// ── 10. Check constraint operations (SQLite returns Err — excluded) ────────

#[test]
fn check_ops() {
    let ops = [
        MigrationOp::AddCheck {
            table: "users".into(),
            check: check("chk_users_age", "age >= 18"),
        },
        MigrationOp::DropCheck {
            table: "users".into(),
            name: "chk_users_old".into(),
            check_def: None,
        },
    ];
    snap("check_ops", &ops, PG_MYSQL);
}

// ── 11. Rename operations ───────────────────────────────────────────────────

#[test]
fn rename_ops() {
    let ops = [
        MigrationOp::RenameTable {
            old_name: "users_old".into(),
            new_name: "users".into(),
        },
        // MySQL CHANGE with full column definition
        MigrationOp::RenameColumn {
            table: "users".into(),
            old_name: "login".into(),
            new_name: "username".into(),
            field_def: Some(field("login", "str").max_len(64).unique()),
        },
        // MySQL fallback path without field_def (emits a warning comment)
        MigrationOp::RenameColumn {
            table: "users".into(),
            old_name: "addr".into(),
            new_name: "address".into(),
            field_def: None,
        },
    ];
    snap("rename_ops", &ops, ALL_DIALECTS);
}

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

use oxyde_codec::ColumnTypeSpec as S;
use oxyde_migrate::{
    CheckDef, Dialect, EnumFieldRef, FieldDef, ForeignKeyDef, IndexDef, Migration, MigrationOp,
    TableDef,
};

/// Shorthand: array spec with the given element.
fn arr(item: S) -> S {
    S::Array {
        item: Box::new(item),
    }
}

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

fn field(name: &str, column_type: S) -> FieldDef {
    FieldDef {
        name: name.into(),
        column_type,
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
        if matches!(self.column_type, S::String { .. }) {
            self.column_type = S::String { length: Some(n) };
        }
        self
    }
    fn decimal(mut self, digits: u32, places: u32) -> Self {
        self.max_digits = Some(digits);
        self.decimal_places = Some(places);
        if matches!(self.column_type, S::Decimal { .. }) {
            self.column_type = S::Decimal {
                precision: Some(digits),
                scale: Some(places),
            };
        }
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
                field("col_int", S::BigInteger),
                field("col_str", S::String { length: None }),
                field("col_str_100", S::String { length: None }).max_len(100),
                field("col_float", S::Double),
                field("col_bool", S::Boolean),
                field("col_bytes", S::Blob),
                field("col_datetime", S::DateTime),
                field("col_date", S::Date),
                field("col_time", S::Time),
                field("col_timedelta", S::Timedelta),
                field("col_uuid", S::Uuid),
                field(
                    "col_decimal",
                    S::Decimal {
                        precision: None,
                        scale: None,
                    },
                ),
                field(
                    "col_decimal_10_2",
                    S::Decimal {
                        precision: None,
                        scale: None,
                    },
                )
                .decimal(10, 2),
                field("col_json", S::Json),
                field("col_unknown", S::Unknown),
                // Arrays
                field("arr_int", arr(S::BigInteger)),
                field("arr_str", arr(S::String { length: None })),
                field("arr_uuid", arr(S::Uuid)),
                field(
                    "arr_decimal",
                    arr(S::Decimal {
                        precision: None,
                        scale: None,
                    }),
                ),
                // Nullability / uniqueness
                field("col_nullable", S::String { length: None }).nullable(),
                field("col_unique", S::String { length: None }).unique(),
                // Defaults (raw SQL fragments)
                field("def_str", S::String { length: None }).default_sql("'hello'"),
                field("def_int", S::BigInteger).default_sql("42"),
                field("def_float", S::Double).default_sql("1.5"),
                field("def_bool", S::Boolean).default_sql("TRUE"),
                field("def_now", S::DateTime).default_sql("CURRENT_TIMESTAMP"),
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
                vec![
                    field("id", S::BigInteger).pk().auto_inc(),
                    field("name", S::String { length: None }),
                ],
            ),
        },
        MigrationOp::CreateTable {
            table: table(
                "t_pk_int_plain",
                vec![
                    field("id", S::BigInteger).pk(),
                    field("name", S::String { length: None }),
                ],
            ),
        },
        MigrationOp::CreateTable {
            table: table(
                "t_pk_uuid",
                vec![
                    field("id", S::Uuid).pk(),
                    field("name", S::String { length: None }),
                ],
            ),
        },
        MigrationOp::CreateTable {
            table: table(
                "t_pk_str",
                vec![
                    field("code", S::String { length: None }).pk().max_len(32),
                    field("name", S::String { length: None }),
                ],
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
                field("c_serial", S::BigInteger).db_type("SERIAL"),
                field("c_bigserial", S::BigInteger).db_type("BIGSERIAL"),
                field("c_jsonb", S::Json).db_type("JSONB"),
                field("c_timestamptz", S::DateTime).db_type("TIMESTAMPTZ"),
                field(
                    "c_numeric",
                    S::Decimal {
                        precision: None,
                        scale: None,
                    },
                )
                .db_type("NUMERIC(10,2)"),
                field("c_varchar", S::String { length: None }).db_type("VARCHAR(100)"),
                field("c_char36", S::Uuid).db_type("CHAR(36)"),
                field("c_money", S::Double).db_type("MONEY"),
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
            field("id", S::BigInteger).pk().auto_inc(),
            field("title", S::String { length: None }),
            field("author_id", S::BigInteger),
            field("editor_id", S::BigInteger).nullable(),
            field("reviewer_id", S::BigInteger).nullable(),
            field("owner_id", S::BigInteger).nullable(),
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
            field: field("nickname", S::String { length: None }).nullable(),
        },
        MigrationOp::AddColumn {
            table: "users".into(),
            field: field("score", S::BigInteger).default_sql("0"),
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
            old_field: field("age", S::BigInteger),
            new_field: field("age", S::String { length: None }).max_len(50),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // NOT NULL → NULL
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("bio", S::String { length: None }),
            new_field: field("bio", S::String { length: None }).nullable(),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // set default
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("status", S::String { length: None }),
            new_field: field("status", S::String { length: None }).default_sql("'active'"),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // drop default
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("rank", S::BigInteger).default_sql("0"),
            new_field: field("rank", S::BigInteger),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // add unique
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("email", S::String { length: None }),
            new_field: field("email", S::String { length: None }).unique(),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // drop unique
        MigrationOp::AlterColumn {
            table: "users".into(),
            old_field: field("phone", S::String { length: None }).unique(),
            new_field: field("phone", S::String { length: None }),
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
        old_field: field("age", S::BigInteger),
        new_field: field("age", S::String { length: None }).max_len(50),
        table_fields: Some(vec![
            field("id", S::BigInteger).pk().auto_inc(),
            field("name", S::String { length: None }),
            field("age", S::BigInteger),
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
            field_def: Some(
                field("login", S::String { length: None })
                    .max_len(64)
                    .unique(),
            ),
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

// ── 12. Enum type operations ────────────────────────────────────────────────

fn enum_spec(name: &str, values: &[&str]) -> S {
    S::Enum {
        name: name.into(),
        values: values.iter().map(|v| (*v).into()).collect(),
    }
}

/// PG: CREATE TYPE (incl. schema-qualified quoting) before CREATE TABLE,
/// native type name for scalar and `[]` for array columns.
/// MySQL: no CREATE TYPE, inline ENUM(...); arrays fall back to JSON.
/// SQLite: TEXT everywhere.
#[test]
fn enum_create_table() {
    let ops = [
        MigrationOp::CreateEnumType {
            name: "post_status_enum".into(),
            values: vec!["draft".into(), "published".into()],
        },
        MigrationOp::CreateEnumType {
            name: "public.review_state_enum".into(),
            values: vec!["open".into(), "closed".into()],
        },
        MigrationOp::CreateTable {
            table: table(
                "enum_posts",
                vec![
                    field("id", S::BigInteger).pk().auto_inc(),
                    field(
                        "status",
                        enum_spec("post_status_enum", &["draft", "published"]),
                    ),
                    field(
                        "labels",
                        arr(enum_spec("post_status_enum", &["draft", "published"])),
                    )
                    .nullable(),
                    field(
                        "review",
                        enum_spec("public.review_state_enum", &["open", "closed"]),
                    ),
                ],
            ),
        },
    ];
    snap("enum_create_table", &ops, ALL_DIALECTS);
}

/// Bucket ordering: AddEnumValue lands before other DDL regardless of the
/// op order in the migration. PG: native ALTER TYPE ... ADD VALUE;
/// MySQL: progressive MODIFY COLUMN per referencing column; SQLite: nothing.
#[test]
fn enum_add_value_ordering() {
    let ops = [
        MigrationOp::CreateIndex {
            table: "enum_posts".into(),
            index: index("ix_enum_posts_status", &["status"]),
        },
        MigrationOp::AddEnumValue {
            name: "post_status_enum".into(),
            value: "archived".into(),
            fields: vec![EnumFieldRef {
                table: "enum_posts".into(),
                field: field(
                    "status",
                    enum_spec("post_status_enum", &["draft", "published", "archived"]),
                ),
            }],
        },
    ];
    snap("enum_add_value_ordering", &ops, ALL_DIALECTS);
}

/// DROP TYPE comes after DROP TABLE (bucket order). AlterEnumType (value
/// removal/reorder) intentionally emits NO SQL on any dialect — execution is
/// guarded by ctx.require_manual() on the Python side; frozen here as-is.
#[test]
fn enum_drop_and_manual_alter() {
    let ops = [
        MigrationOp::DropEnumType {
            name: "post_status_enum".into(),
            values: Some(vec!["draft".into(), "published".into()]),
        },
        MigrationOp::DropTable {
            name: "enum_posts".into(),
            table: None,
        },
        MigrationOp::AlterEnumType {
            name: "review_state_enum".into(),
            old_values: vec!["open".into(), "closed".into()],
            new_values: vec!["open".into()],
        },
    ];
    snap("enum_drop_and_manual_alter", &ops, ALL_DIALECTS);
}

/// Column type conversion to/from an enum. PG needs the `::text::<target>`
/// USING bridge (no implicit cast in either direction) — this is the 0.6→0.7
/// upgrade path for pre-existing str-Enum fields stored as TEXT.
/// SQLite goes through table rebuild — excluded here.
#[test]
fn alter_column_enum() {
    let ops = [
        // TEXT → enum (upgrade path)
        MigrationOp::AlterColumn {
            table: "posts".into(),
            old_field: field("status", S::Text),
            new_field: field(
                "status",
                enum_spec("post_status_enum", &["draft", "published"]),
            ),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
        // enum → TEXT (user dropped the enum annotation)
        MigrationOp::AlterColumn {
            table: "posts".into(),
            old_field: field("kind", enum_spec("post_kind_enum", &["news", "blog"])),
            new_field: field("kind", S::Text),
            table_fields: None,
            table_indexes: None,
            table_foreign_keys: None,
            table_checks: None,
        },
    ];
    snap("alter_column_enum", &ops, PG_MYSQL);
}

//! SQL generation using sea-query DDL builders.
//!
//! Type mapping (`python_type_to_sql`, `translate_db_type`, `resolve_field_type`) is
//! hand-written — sea-query doesn't know about Python types. DDL structure
//! (CREATE/ALTER/DROP TABLE, indexes, foreign keys) uses sea-query for
//! dialect-specific syntax and identifier quoting.

use sea_query::{
    Alias, ColumnDef as SeaColumnDef, Expr, ForeignKey as SeaForeignKey,
    ForeignKeyAction as SeaFkAction, Index as SeaIndex, IndexType, IntoIden, MysqlQueryBuilder,
    PostgresQueryBuilder, SqliteQueryBuilder, Table as SeaTable,
};

use crate::op::MigrationOp;
use crate::types::{CheckDef, Dialect, FieldDef, ForeignKeyDef, IndexDef, MigrateError, Result};

/// Build SQL string from a sea-query schema statement for the given dialect.
macro_rules! build_sql {
    ($stmt:expr, $dialect:expr) => {
        match $dialect {
            Dialect::Sqlite => $stmt.build(SqliteQueryBuilder),
            Dialect::Postgres => $stmt.build(PostgresQueryBuilder),
            Dialect::Mysql => $stmt.build(MysqlQueryBuilder),
        }
    };
}

// ── Type mapping ────────────────────────────────────────────────────────────

/// Generate SQL type from Python type name for a given dialect.
///
/// Used when `db_type` is not explicitly specified by the user.
pub(crate) fn python_type_to_sql(python_type: &str, dialect: Dialect, is_pk: bool) -> String {
    // Handle array types: "int[]", "str[]", "uuid[]", etc.
    if let Some(inner) = python_type.strip_suffix("[]") {
        let inner_sql = python_type_to_sql(inner, dialect, false);
        return match dialect {
            Dialect::Postgres => format!("{}[]", inner_sql),
            // MySQL/SQLite: no native arrays, use JSON
            Dialect::Mysql => "JSON".to_string(),
            Dialect::Sqlite => "TEXT".to_string(),
        };
    }

    match dialect {
        Dialect::Sqlite => match python_type {
            "int" => "INTEGER".to_string(),
            "str" => "TEXT".to_string(),
            "float" => "REAL".to_string(),
            "bool" => "INTEGER".to_string(),
            "bytes" => "BLOB".to_string(),
            "datetime" => "TEXT".to_string(),
            "date" => "TEXT".to_string(),
            "time" => "TEXT".to_string(),
            "timedelta" => "BIGINT".to_string(),
            "uuid" => "TEXT".to_string(),
            "decimal" => "TEXT".to_string(),
            "json" => "TEXT".to_string(),
            _ => "TEXT".to_string(),
        },
        Dialect::Postgres => match python_type {
            "int" if is_pk => "BIGSERIAL".to_string(),
            "int" => "BIGINT".to_string(),
            "str" => "TEXT".to_string(),
            "float" => "DOUBLE PRECISION".to_string(),
            "bool" => "BOOLEAN".to_string(),
            "bytes" => "BYTEA".to_string(),
            "datetime" => "TIMESTAMP".to_string(),
            "date" => "DATE".to_string(),
            "time" => "TIME".to_string(),
            "timedelta" => "BIGINT".to_string(),
            "uuid" => "UUID".to_string(),
            "decimal" => "NUMERIC".to_string(),
            "json" => "JSONB".to_string(),
            _ => "TEXT".to_string(),
        },
        Dialect::Mysql => match python_type {
            "int" => "BIGINT".to_string(),
            "str" => "TEXT".to_string(),
            "float" => "DOUBLE".to_string(),
            "bool" => "TINYINT".to_string(),
            "bytes" => "BLOB".to_string(),
            "datetime" => "DATETIME(6)".to_string(),
            "date" => "DATE".to_string(),
            "time" => "TIME(6)".to_string(),
            "timedelta" => "BIGINT".to_string(),
            "uuid" => "CHAR(36)".to_string(),
            "decimal" => "DECIMAL".to_string(),
            "json" => "JSON".to_string(),
            _ => "TEXT".to_string(),
        },
    }
}

/// Translate database-specific types for cross-platform compatibility.
///
/// E.g., SERIAL/BIGSERIAL (PostgreSQL) → INT/BIGINT (MySQL) → INTEGER (SQLite)
pub(crate) fn translate_db_type(db_type: &str, dialect: Dialect) -> String {
    let db_type_upper = db_type.to_uppercase();

    match dialect {
        Dialect::Sqlite => match db_type_upper.as_str() {
            "SERIAL" | "BIGSERIAL" => "INTEGER".to_string(),
            _ => db_type.to_string(),
        },
        Dialect::Mysql => match db_type_upper.as_str() {
            "SERIAL" => "INT".to_string(),
            "BIGSERIAL" => "BIGINT".to_string(),
            _ => db_type.to_string(),
        },
        Dialect::Postgres => db_type.to_string(),
    }
}

/// Resolve the SQL type for a field based on dialect.
///
/// Priority:
/// 1. If `db_type` is set (user explicit) → translate for dialect
/// 2. Generate from `python_type` for dialect
fn resolve_field_type(field: &FieldDef, dialect: Dialect) -> String {
    if let Some(db_type) = &field.db_type {
        return translate_db_type(db_type, dialect);
    }
    // str → VARCHAR(N) on PG/MySQL, TEXT on SQLite
    if field.python_type == "str" && dialect != Dialect::Sqlite {
        let len = field.max_length.unwrap_or(255);
        return format!("VARCHAR({})", len);
    }
    // decimal → DECIMAL(M,D) on MySQL when constraints are specified
    if field.python_type == "decimal" {
        if let Some(digits) = field.max_digits {
            let places = field.decimal_places.unwrap_or(0);
            return match dialect {
                Dialect::Mysql => format!("DECIMAL({},{})", digits, places),
                Dialect::Postgres => format!("NUMERIC({},{})", digits, places),
                Dialect::Sqlite => "TEXT".to_string(),
            };
        }
    }
    python_type_to_sql(&field.python_type, dialect, field.primary_key)
}

// ── sea-query helpers ───────────────────────────────────────────────────────

/// Convert `FieldDef` to sea-query `ColumnDef` with dialect-appropriate type and constraints.
fn field_to_column_def(field: &FieldDef, dialect: Dialect) -> SeaColumnDef {
    let sql_type = resolve_field_type(field, dialect);
    let mut col = SeaColumnDef::new(Alias::new(&field.name));
    col.custom(Alias::new(sql_type));

    // SQLite requires "PRIMARY KEY AUTOINCREMENT" in that exact order.
    // sea-query's .extra() renders before .primary_key(), so for this case
    // we emit both keywords via .extra() and skip .primary_key().
    let sqlite_autoincrement =
        field.primary_key && field.auto_increment && dialect == Dialect::Sqlite;

    if field.primary_key && !sqlite_autoincrement {
        col.primary_key();
    }

    if field.auto_increment {
        match dialect {
            Dialect::Sqlite => {
                if field.primary_key {
                    col.extra("PRIMARY KEY AUTOINCREMENT");
                }
            }
            Dialect::Mysql => {
                col.extra("AUTO_INCREMENT");
            }
            Dialect::Postgres => {} // SERIAL type handles auto-increment
        }
    }

    if !field.nullable && !field.primary_key {
        col.not_null();
    }

    if field.unique && !field.primary_key {
        col.unique_key();
    }

    if let Some(default) = &field.default {
        col.default(Expr::cust(default));
    }

    col
}

/// Parse FK action string to sea-query `ForeignKeyAction`.
fn parse_fk_action(action: Option<&str>) -> SeaFkAction {
    match action.unwrap_or("NO ACTION").to_uppercase().as_str() {
        "CASCADE" => SeaFkAction::Cascade,
        "SET NULL" => SeaFkAction::SetNull,
        "SET DEFAULT" => SeaFkAction::SetDefault,
        "RESTRICT" => SeaFkAction::Restrict,
        _ => SeaFkAction::NoAction,
    }
}

/// Build a sea-query `ForeignKeyCreateStatement` from `ForeignKeyDef`.
fn build_fk_stmt(table: &str, fk: &ForeignKeyDef) -> sea_query::ForeignKeyCreateStatement {
    let mut stmt = SeaForeignKey::create();
    stmt.name(&fk.name)
        .from_tbl(Alias::new(table))
        .to_tbl(Alias::new(&fk.ref_table))
        .on_delete(parse_fk_action(fk.on_delete.as_deref()))
        .on_update(parse_fk_action(fk.on_update.as_deref()));
    for col in &fk.columns {
        stmt.from_col(Alias::new(col));
    }
    for col in &fk.ref_columns {
        stmt.to_col(Alias::new(col));
    }
    stmt
}

/// Build a MySQL column definition string (backtick-quoted name + type + constraints).
///
/// Used by both `RenameColumn` (CHANGE) and `AlterColumn` (MODIFY COLUMN).
fn mysql_column_def(field: &FieldDef) -> String {
    let sql_type = resolve_field_type(field, Dialect::Mysql);
    let mut col_def = format!("`{}` {}", field.name, sql_type);

    if field.primary_key {
        col_def.push_str(" PRIMARY KEY");
    }
    if field.auto_increment {
        col_def.push_str(" AUTO_INCREMENT");
    }
    if !field.nullable && !field.primary_key {
        col_def.push_str(" NOT NULL");
    }
    if field.unique && !field.primary_key {
        col_def.push_str(" UNIQUE");
    }
    if let Some(default) = &field.default {
        col_def.push_str(&format!(" DEFAULT {}", default));
    }

    col_def
}

/// Build CREATE INDEX SQL for an index on a table.
fn build_create_index(table: &str, index: &IndexDef, dialect: Dialect) -> String {
    let mut stmt = SeaIndex::create();
    stmt.name(&index.name).table(Alias::new(table));

    if index.unique {
        stmt.unique();
    }

    for field in &index.fields {
        stmt.col(Alias::new(field));
    }

    // Index method (USING btree/hash/gin/gist) — Postgres only
    if dialect == Dialect::Postgres {
        if let Some(method) = &index.method {
            stmt.index_type(IndexType::Custom(Alias::new(method).into_iden()));
        }
    }

    build_sql!(stmt, dialect)
}

// ── SQLite table rebuild ────────────────────────────────────────────────────

/// SQLite doesn't support ALTER COLUMN — rebuild the entire table.
///
/// 1. `PRAGMA foreign_keys=OFF`
/// 2. CREATE TABLE `_new_X` with updated schema
/// 3. Copy data from old table
/// 4. DROP old table
/// 5. RENAME new table
/// 6. Recreate indexes
/// 7. `PRAGMA foreign_keys=ON`
fn sqlite_table_rebuild(
    table: &str,
    fields: &[FieldDef],
    indexes: &[IndexDef],
    foreign_keys: &[ForeignKeyDef],
    checks: &[CheckDef],
    altered_column: &str,
    new_field: &FieldDef,
) -> Result<Vec<String>> {
    let mut stmts = Vec::new();
    let temp_table = format!("_new_{}", table);

    stmts.push("PRAGMA foreign_keys=OFF".to_string());

    // Build new table with sea-query
    let mut create = SeaTable::create();
    create.table(Alias::new(&temp_table));

    let mut column_names = Vec::new();
    for field in fields {
        let col = if field.name == altered_column {
            field_to_column_def(new_field, Dialect::Sqlite)
        } else {
            field_to_column_def(field, Dialect::Sqlite)
        };
        create.col(col);
        column_names.push(field.name.clone());
    }

    // Inline FK constraints (SQLite requirement)
    for fk in foreign_keys {
        let mut fk_stmt = build_fk_stmt(&temp_table, fk);
        create.foreign_key(&mut fk_stmt);
    }

    // Inline CHECK constraints
    for check in checks {
        create.check(Expr::cust(&check.expression));
    }

    stmts.push(create.build(SqliteQueryBuilder));

    // Copy data
    let columns = column_names.join(", ");
    stmts.push(format!(
        "INSERT INTO \"{temp_table}\" ({columns}) SELECT {columns} FROM \"{table}\""
    ));

    // Drop old table
    stmts.push(
        SeaTable::drop()
            .table(Alias::new(table))
            .build(SqliteQueryBuilder),
    );

    // Rename temp → original
    stmts.push(
        SeaTable::rename()
            .table(Alias::new(&temp_table), Alias::new(table))
            .build(SqliteQueryBuilder),
    );

    // Recreate indexes
    for index in indexes {
        stmts.push(build_create_index(table, index, Dialect::Sqlite));
    }

    stmts.push("PRAGMA foreign_keys=ON".to_string());

    Ok(stmts)
}

// ── MigrationOp::to_sql ────────────────────────────────────────────────────

impl MigrationOp {
    /// Generate SQL for this migration operation.
    ///
    /// Returns `Err` for operations not supported by the dialect
    /// (e.g., ALTER COLUMN on SQLite without table schema).
    pub fn to_sql(&self, dialect: Dialect) -> Result<Vec<String>> {
        match self {
            MigrationOp::CreateTable { table } => {
                let mut create = SeaTable::create();
                create.table(Alias::new(&table.name));

                for field in &table.fields {
                    create.col(field_to_column_def(field, dialect));
                }

                // SQLite: FK and CHECK must be inline in CREATE TABLE
                if dialect == Dialect::Sqlite {
                    for fk in &table.foreign_keys {
                        let mut fk_stmt = build_fk_stmt(&table.name, fk);
                        create.foreign_key(&mut fk_stmt);
                    }
                    for check in &table.checks {
                        create.check(Expr::cust(&check.expression));
                    }
                }

                let mut sql = vec![build_sql!(create, dialect)];

                // Indexes (all dialects)
                for index in &table.indexes {
                    sql.push(build_create_index(&table.name, index, dialect));
                }

                // PG/MySQL: FK and CHECK as separate ALTER TABLE statements
                // (handles circular dependencies between tables)
                if dialect != Dialect::Sqlite {
                    for fk in &table.foreign_keys {
                        sql.push(build_sql!(build_fk_stmt(&table.name, fk), dialect));
                    }
                    for check in &table.checks {
                        sql.push(format!(
                            "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({})",
                            table.name, check.name, check.expression
                        ));
                    }
                }

                Ok(sql)
            }

            MigrationOp::DropTable { name, table: _ } => Ok(vec![build_sql!(
                SeaTable::drop().table(Alias::new(name)),
                dialect
            )]),

            MigrationOp::RenameTable { old_name, new_name } => Ok(vec![build_sql!(
                SeaTable::rename().table(Alias::new(old_name), Alias::new(new_name)),
                dialect
            )]),

            MigrationOp::AddColumn { table, field } => {
                let col = field_to_column_def(field, dialect);
                Ok(vec![build_sql!(
                    SeaTable::alter().table(Alias::new(table)).add_column(col),
                    dialect
                )])
            }

            MigrationOp::DropColumn {
                table,
                field,
                field_def: _,
            } => Ok(vec![build_sql!(
                SeaTable::alter()
                    .table(Alias::new(table))
                    .drop_column(Alias::new(field)),
                dialect
            )]),

            MigrationOp::RenameColumn {
                table,
                old_name,
                new_name,
                field_def,
            } => {
                Ok(match dialect {
                    Dialect::Mysql => {
                        // MySQL CHANGE requires full column definition
                        if let Some(field) = field_def {
                            let mut renamed = field.clone();
                            renamed.name = new_name.clone();
                            vec![format!(
                                "ALTER TABLE `{}` CHANGE `{}` {}",
                                table,
                                old_name,
                                mysql_column_def(&renamed)
                            )]
                        } else {
                            vec![
                                format!("-- WARNING: field_def not provided, column attributes may be lost"),
                                format!("ALTER TABLE `{}` CHANGE `{}` `{}` TEXT", table, old_name, new_name),
                            ]
                        }
                    }
                    _ => vec![build_sql!(
                        SeaTable::alter()
                            .table(Alias::new(table))
                            .rename_column(Alias::new(old_name), Alias::new(new_name)),
                        dialect
                    )],
                })
            }

            MigrationOp::AlterColumn {
                table,
                old_field,
                new_field,
                table_fields,
                table_indexes,
                table_foreign_keys,
                table_checks,
            } => match dialect {
                Dialect::Postgres => {
                    let mut stmts = Vec::new();
                    let old_sql_type = resolve_field_type(old_field, dialect);
                    let new_sql_type = resolve_field_type(new_field, dialect);

                    if old_sql_type != new_sql_type {
                        stmts.push(format!(
                            "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" TYPE {}",
                            table, new_field.name, new_sql_type
                        ));
                    }

                    if old_field.nullable != new_field.nullable {
                        let null_action = if new_field.nullable {
                            "DROP NOT NULL"
                        } else {
                            "SET NOT NULL"
                        };
                        stmts.push(format!(
                            "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" {}",
                            table, new_field.name, null_action
                        ));
                    }

                    if old_field.default != new_field.default {
                        if let Some(default) = &new_field.default {
                            stmts.push(format!(
                                "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" SET DEFAULT {}",
                                table, new_field.name, default
                            ));
                        } else {
                            stmts.push(format!(
                                "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" DROP DEFAULT",
                                table, new_field.name
                            ));
                        }
                    }

                    if old_field.unique != new_field.unique {
                        if new_field.unique {
                            stmts.push(format!(
                                "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}_{}_key\" UNIQUE (\"{}\")",
                                table, table, new_field.name, new_field.name
                            ));
                        } else {
                            stmts.push(format!(
                                "ALTER TABLE \"{}\" DROP CONSTRAINT \"{}_{}_key\"",
                                table, table, new_field.name
                            ));
                        }
                    }

                    Ok(stmts)
                }
                Dialect::Mysql => {
                    // MySQL: MODIFY COLUMN with full column definition
                    Ok(vec![format!(
                        "ALTER TABLE `{}` MODIFY COLUMN {}",
                        table,
                        mysql_column_def(new_field)
                    )])
                }
                Dialect::Sqlite => {
                    if let Some(fields) = table_fields {
                        sqlite_table_rebuild(
                            table,
                            fields,
                            table_indexes.as_deref().unwrap_or(&[]),
                            table_foreign_keys.as_deref().unwrap_or(&[]),
                            table_checks.as_deref().unwrap_or(&[]),
                            &old_field.name,
                            new_field,
                        )
                    } else {
                        Err(MigrateError::MigrationError(format!(
                            "SQLite does not support ALTER COLUMN. Table '{}' column '{}' requires table rebuild. \
                            Provide table_fields for automatic rebuild, or use manual migration: \
                            1) CREATE TABLE {}_new with new schema, \
                            2) INSERT INTO {}_new SELECT * FROM {}, \
                            3) DROP TABLE {}, \
                            4) ALTER TABLE {}_new RENAME TO {}",
                            table, new_field.name,
                            table, table, table, table, table, table
                        )))
                    }
                }
            },

            MigrationOp::CreateIndex { table, index } => {
                Ok(vec![build_create_index(table, index, dialect)])
            }

            MigrationOp::DropIndex {
                table,
                index,
                index_def: _,
            } => {
                let mut stmt = SeaIndex::drop();
                stmt.name(index).table(Alias::new(table));
                Ok(vec![build_sql!(stmt, dialect)])
            }

            MigrationOp::AddForeignKey { table, fk } => {
                if dialect == Dialect::Sqlite {
                    return Err(MigrateError::MigrationError(format!(
                        "SQLite does not support ALTER TABLE ADD FOREIGN KEY. \
                        To add a foreign key to table '{}', you need to recreate the table. \
                        Consider using a table rebuild migration.",
                        table
                    )));
                }
                Ok(vec![build_sql!(build_fk_stmt(table, fk), dialect)])
            }

            MigrationOp::DropForeignKey {
                table,
                name,
                fk_def: _,
            } => {
                if dialect == Dialect::Sqlite {
                    return Err(MigrateError::MigrationError(format!(
                        "SQLite does not support ALTER TABLE DROP FOREIGN KEY. \
                        To remove foreign key '{}' from table '{}', you need to recreate the table. \
                        Consider using a table rebuild migration.",
                        name, table
                    )));
                }

                let mut stmt = SeaForeignKey::drop();
                stmt.name(name).table(Alias::new(table));
                Ok(vec![build_sql!(stmt, dialect)])
            }

            MigrationOp::AddCheck { table, check } => {
                if dialect == Dialect::Sqlite {
                    return Err(MigrateError::MigrationError(format!(
                        "SQLite does not support ALTER TABLE ADD CHECK. \
                        To add a check constraint to table '{}', you need to recreate the table. \
                        Consider using a table rebuild migration.",
                        table
                    )));
                }
                Ok(vec![format!(
                    "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({})",
                    table, check.name, check.expression
                )])
            }

            MigrationOp::DropCheck {
                table,
                name,
                check_def: _,
            } => {
                if dialect == Dialect::Sqlite {
                    return Err(MigrateError::MigrationError(format!(
                        "SQLite does not support ALTER TABLE DROP CHECK. \
                        To remove check constraint '{}' from table '{}', you need to recreate the table. \
                        Consider using a table rebuild migration.",
                        name, table
                    )));
                }
                Ok(match dialect {
                    Dialect::Mysql => vec![format!("ALTER TABLE {} DROP CHECK {}", table, name)],
                    Dialect::Postgres => {
                        vec![format!("ALTER TABLE {} DROP CONSTRAINT {}", table, name)]
                    }
                    Dialect::Sqlite => unreachable!(),
                })
            }
        }
    }
}

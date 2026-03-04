//! SQL generation: type mapping, column definitions, and MigrationOp::to_sql().

use crate::op::MigrationOp;
use crate::types::{CheckDef, Dialect, FieldDef, ForeignKeyDef, IndexDef, MigrateError, Result};

/// Generate SQL type from Python type name for a given dialect.
///
/// This is used when db_type is not explicitly specified by the user.
pub(crate) fn python_type_to_sql(python_type: &str, dialect: Dialect, is_pk: bool) -> String {
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
            "timedelta" => "TEXT".to_string(),
            "uuid" => "TEXT".to_string(),
            "decimal" => "NUMERIC".to_string(),
            _ => "TEXT".to_string(),
        },
        Dialect::Postgres => match python_type {
            "int" if is_pk => "SERIAL".to_string(),
            "int" => "BIGINT".to_string(),
            "str" => "TEXT".to_string(),
            "float" => "DOUBLE PRECISION".to_string(),
            "bool" => "BOOLEAN".to_string(),
            "bytes" => "BYTEA".to_string(),
            "datetime" => "TIMESTAMP".to_string(),
            "date" => "DATE".to_string(),
            "time" => "TIME".to_string(),
            "timedelta" => "INTERVAL".to_string(),
            "uuid" => "UUID".to_string(),
            "decimal" => "NUMERIC".to_string(),
            _ => "TEXT".to_string(),
        },
        Dialect::Mysql => match python_type {
            "int" => "BIGINT".to_string(),
            "str" => "TEXT".to_string(),
            "float" => "DOUBLE".to_string(),
            "bool" => "TINYINT".to_string(),
            "bytes" => "BLOB".to_string(),
            "datetime" => "DATETIME".to_string(),
            "date" => "DATE".to_string(),
            "time" => "TIME".to_string(),
            "timedelta" => "TIME".to_string(),
            "uuid" => "CHAR(36)".to_string(),
            "decimal" => "DECIMAL".to_string(),
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
/// 1. If db_type is set (user explicit) → translate for dialect
/// 2. Generate from python_type for dialect
fn resolve_field_type(field: &FieldDef, dialect: Dialect) -> String {
    // 1. Explicit db_type from user - translate if needed
    if let Some(db_type) = &field.db_type {
        return translate_db_type(db_type, dialect);
    }

    // 2. Generate from python_type
    python_type_to_sql(&field.python_type, dialect, field.primary_key)
}

/// Build column definition from FieldDef for any dialect
fn build_column_def(field: &FieldDef, dialect: Dialect) -> String {
    let sql_type = resolve_field_type(field, dialect);
    let mut col_def = format!("{} {}", field.name, sql_type);

    if field.primary_key {
        col_def.push_str(" PRIMARY KEY");
    }

    // Handle auto_increment per dialect
    if field.auto_increment {
        match dialect {
            Dialect::Sqlite => {
                if field.primary_key {
                    col_def.push_str(" AUTOINCREMENT");
                }
            }
            Dialect::Mysql => col_def.push_str(" AUTO_INCREMENT"),
            // Postgres uses SERIAL/BIGSERIAL types, no separate keyword needed
            Dialect::Postgres => {}
        }
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

/// Generate SQLite table rebuild SQL for ALTER COLUMN operation
///
/// SQLite doesn't support ALTER COLUMN, so we need to:
/// 1. Disable foreign keys
/// 2. Create new table with updated schema (including FK/CHECK inline)
/// 3. Copy data from old table
/// 4. Drop old table
/// 5. Rename new table to original name
/// 6. Recreate indexes
/// 7. Re-enable foreign keys
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

    // 1. Disable foreign keys
    stmts.push("PRAGMA foreign_keys=OFF".to_string());

    // 2. Build new table schema with altered column
    let mut table_parts = Vec::new();
    let mut column_names = Vec::new();

    for field in fields {
        if field.name == altered_column {
            // Use the new field definition
            table_parts.push(build_column_def(new_field, Dialect::Sqlite));
        } else {
            table_parts.push(build_column_def(field, Dialect::Sqlite));
        }
        column_names.push(field.name.clone());
    }

    // Add foreign key constraints inline (SQLite requirement)
    for fk in foreign_keys {
        let on_delete = fk.on_delete.as_deref().unwrap_or("NO ACTION");
        let on_update = fk.on_update.as_deref().unwrap_or("NO ACTION");

        table_parts.push(format!(
            "FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
            fk.columns.join(", "),
            fk.ref_table,
            fk.ref_columns.join(", "),
            on_delete,
            on_update
        ));
    }

    // Add check constraints inline (SQLite requirement)
    for check in checks {
        table_parts.push(format!("CHECK ({})", check.expression));
    }

    stmts.push(format!(
        "CREATE TABLE {} ({})",
        temp_table,
        table_parts.join(", ")
    ));

    // 3. Copy data from old table to new table
    let columns = column_names.join(", ");
    stmts.push(format!(
        "INSERT INTO {} ({}) SELECT {} FROM {}",
        temp_table, columns, columns, table
    ));

    // 4. Drop old table
    stmts.push(format!("DROP TABLE {}", table));

    // 5. Rename new table to original name
    stmts.push(format!("ALTER TABLE {} RENAME TO {}", temp_table, table));

    // 6. Recreate indexes
    for index in indexes {
        let unique = if index.unique { "UNIQUE " } else { "" };
        stmts.push(format!(
            "CREATE {}INDEX {} ON {} ({})",
            unique,
            index.name,
            table,
            index.fields.join(", ")
        ));
    }

    // 7. Re-enable foreign keys
    stmts.push("PRAGMA foreign_keys=ON".to_string());

    Ok(stmts)
}

impl MigrationOp {
    /// Generate SQL for this operation
    /// Returns Err for operations not supported by the dialect (e.g., ALTER COLUMN on SQLite)
    pub fn to_sql(&self, dialect: Dialect) -> Result<Vec<String>> {
        match self {
            MigrationOp::CreateTable { table } => {
                let mut fields_sql: Vec<String> = table
                    .fields
                    .iter()
                    .map(|field| build_column_def(field, dialect))
                    .collect();

                // For SQLite: FK and CHECK constraints must be inline in CREATE TABLE
                // (SQLite doesn't support ALTER TABLE ADD CONSTRAINT)
                if dialect == Dialect::Sqlite {
                    // Add foreign key constraints inline
                    for fk in &table.foreign_keys {
                        let on_delete = fk.on_delete.as_deref().unwrap_or("NO ACTION");
                        let on_update = fk.on_update.as_deref().unwrap_or("NO ACTION");

                        fields_sql.push(format!(
                            "FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
                            fk.columns.join(", "),
                            fk.ref_table,
                            fk.ref_columns.join(", "),
                            on_delete,
                            on_update
                        ));
                    }

                    // Add check constraints inline
                    for check in &table.checks {
                        fields_sql.push(format!("CHECK ({})", check.expression));
                    }
                }

                let mut sql = vec![format!(
                    "CREATE TABLE {} ({})",
                    table.name,
                    fields_sql.join(", ")
                )];

                // Add indexes (works the same for all dialects)
                for index in &table.indexes {
                    let unique = if index.unique { "UNIQUE " } else { "" };

                    // MySQL and Postgres support USING, SQLite doesn't
                    let method = match dialect {
                        Dialect::Postgres => index
                            .method
                            .as_ref()
                            .map(|m| format!(" USING {}", m))
                            .unwrap_or_default(),
                        _ => String::new(),
                    };

                    sql.push(format!(
                        "CREATE {}INDEX {} ON {} ({}){}",
                        unique,
                        index.name,
                        table.name,
                        index.fields.join(", "),
                        method
                    ));
                }

                // For PostgreSQL/MySQL: Add foreign keys as separate ALTER TABLE
                // (allows handling circular dependencies between tables)
                if dialect != Dialect::Sqlite {
                    for fk in &table.foreign_keys {
                        let on_delete = fk.on_delete.as_deref().unwrap_or("NO ACTION");
                        let on_update = fk.on_update.as_deref().unwrap_or("NO ACTION");

                        sql.push(format!(
                            "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
                            table.name,
                            fk.name,
                            fk.columns.join(", "),
                            fk.ref_table,
                            fk.ref_columns.join(", "),
                            on_delete,
                            on_update
                        ));
                    }

                    // Add check constraints
                    for check in &table.checks {
                        sql.push(format!(
                            "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({})",
                            table.name, check.name, check.expression
                        ));
                    }
                }

                Ok(sql)
            }
            MigrationOp::DropTable { name, table: _ } => Ok(vec![format!("DROP TABLE {}", name)]),
            MigrationOp::RenameTable { old_name, new_name } => Ok(match dialect {
                Dialect::Mysql => vec![format!("RENAME TABLE {} TO {}", old_name, new_name)],
                _ => vec![format!("ALTER TABLE {} RENAME TO {}", old_name, new_name)],
            }),
            MigrationOp::AddColumn { table, field } => {
                let sql_type = resolve_field_type(field, dialect);
                let mut field_sql = format!("{} {}", field.name, sql_type);

                if !field.nullable {
                    field_sql.push_str(" NOT NULL");
                }

                if field.unique {
                    field_sql.push_str(" UNIQUE");
                }

                if let Some(default) = &field.default {
                    field_sql.push_str(&format!(" DEFAULT {}", default));
                }

                Ok(vec![format!(
                    "ALTER TABLE {} ADD COLUMN {}",
                    table, field_sql
                )])
            }
            MigrationOp::DropColumn {
                table,
                field,
                field_def: _,
            } => Ok(vec![format!("ALTER TABLE {} DROP COLUMN {}", table, field)]),
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
                            // Build full definition with new name
                            let mut renamed_field = field.clone();
                            renamed_field.name = new_name.clone();
                            let col_def = build_column_def(&renamed_field, dialect);
                            vec![format!(
                                "ALTER TABLE {} CHANGE {} {}",
                                table, old_name, col_def
                            )]
                        } else {
                            // Fallback: just type (loses attributes - emit warning comment)
                            vec![
                                format!("-- WARNING: field_def not provided, column attributes may be lost"),
                                format!("ALTER TABLE {} CHANGE {} {} TEXT", table, old_name, new_name),
                            ]
                        }
                    }
                    Dialect::Postgres => vec![format!(
                        "ALTER TABLE {} RENAME COLUMN {} TO {}",
                        table, old_name, new_name
                    )],
                    Dialect::Sqlite => vec![format!(
                        "ALTER TABLE {} RENAME COLUMN {} TO {}",
                        table, old_name, new_name
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
            } => {
                match dialect {
                    Dialect::Postgres => {
                        // PostgreSQL: multiple ALTER statements for type, null, default
                        let mut stmts = Vec::new();

                        // Resolve types for comparison and SQL generation
                        let old_sql_type = resolve_field_type(old_field, dialect);
                        let new_sql_type = resolve_field_type(new_field, dialect);

                        // Change type if different
                        if old_sql_type != new_sql_type {
                            stmts.push(format!(
                                "ALTER TABLE {} ALTER COLUMN {} TYPE {}",
                                table, new_field.name, new_sql_type
                            ));
                        }

                        // Change nullability if different
                        if old_field.nullable != new_field.nullable {
                            let null_action = if new_field.nullable {
                                "DROP NOT NULL"
                            } else {
                                "SET NOT NULL"
                            };
                            stmts.push(format!(
                                "ALTER TABLE {} ALTER COLUMN {} {}",
                                table, new_field.name, null_action
                            ));
                        }

                        // Change default if different
                        if old_field.default != new_field.default {
                            if let Some(default) = &new_field.default {
                                stmts.push(format!(
                                    "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {}",
                                    table, new_field.name, default
                                ));
                            } else {
                                stmts.push(format!(
                                    "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT",
                                    table, new_field.name
                                ));
                            }
                        }

                        // Change unique constraint if different
                        if old_field.unique != new_field.unique {
                            if new_field.unique {
                                // Add unique constraint
                                stmts.push(format!(
                                    "ALTER TABLE {} ADD CONSTRAINT {}_{}_key UNIQUE ({})",
                                    table, table, new_field.name, new_field.name
                                ));
                            } else {
                                // Drop unique constraint
                                stmts.push(format!(
                                    "ALTER TABLE {} DROP CONSTRAINT {}_{}_key",
                                    table, table, new_field.name
                                ));
                            }
                        }

                        Ok(stmts)
                    }
                    Dialect::Mysql => {
                        // MySQL: MODIFY COLUMN with full column definition
                        let col_def = build_column_def(new_field, dialect);
                        Ok(vec![format!(
                            "ALTER TABLE {} MODIFY COLUMN {}",
                            table, col_def
                        )])
                    }
                    Dialect::Sqlite => {
                        // SQLite: table rebuild if we have full schema
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
                            // No schema provided - return explicit error
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
                }
            }
            MigrationOp::CreateIndex { table, index } => {
                let unique = if index.unique { "UNIQUE " } else { "" };
                let method = index
                    .method
                    .as_ref()
                    .map(|m| format!(" USING {}", m))
                    .unwrap_or_default();

                Ok(vec![format!(
                    "CREATE {}INDEX {} ON {} ({}){}",
                    unique,
                    index.name,
                    table,
                    index.fields.join(", "),
                    method
                )])
            }
            MigrationOp::DropIndex {
                table,
                index,
                index_def: _,
            } => Ok(match dialect {
                Dialect::Mysql => vec![format!("DROP INDEX {} ON {}", index, table)],
                _ => vec![format!("DROP INDEX {}", index)],
            }),
            MigrationOp::AddForeignKey { table, fk } => {
                // SQLite doesn't support ALTER TABLE ADD CONSTRAINT for foreign keys
                if dialect == Dialect::Sqlite {
                    return Err(MigrateError::MigrationError(format!(
                        "SQLite does not support ALTER TABLE ADD FOREIGN KEY. \
                        To add a foreign key to table '{}', you need to recreate the table. \
                        Consider using a table rebuild migration.",
                        table
                    )));
                }

                let on_delete = fk.on_delete.as_deref().unwrap_or("NO ACTION");
                let on_update = fk.on_update.as_deref().unwrap_or("NO ACTION");

                Ok(vec![format!(
                    "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
                    table,
                    fk.name,
                    fk.columns.join(", "),
                    fk.ref_table,
                    fk.ref_columns.join(", "),
                    on_delete,
                    on_update
                )])
            }
            MigrationOp::DropForeignKey {
                table,
                name,
                fk_def: _,
            } => {
                // SQLite doesn't support ALTER TABLE DROP CONSTRAINT
                if dialect == Dialect::Sqlite {
                    return Err(MigrateError::MigrationError(format!(
                        "SQLite does not support ALTER TABLE DROP FOREIGN KEY. \
                        To remove foreign key '{}' from table '{}', you need to recreate the table. \
                        Consider using a table rebuild migration.",
                        name, table
                    )));
                }

                Ok(match dialect {
                    // MySQL uses DROP FOREIGN KEY
                    Dialect::Mysql => {
                        vec![format!("ALTER TABLE {} DROP FOREIGN KEY {}", table, name)]
                    }
                    // PostgreSQL uses DROP CONSTRAINT
                    Dialect::Postgres => {
                        vec![format!("ALTER TABLE {} DROP CONSTRAINT {}", table, name)]
                    }
                    // SQLite case handled above
                    Dialect::Sqlite => unreachable!(),
                })
            }
            MigrationOp::AddCheck { table, check } => {
                // SQLite doesn't support ALTER TABLE ADD CONSTRAINT for check constraints
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
                // SQLite doesn't support ALTER TABLE DROP CONSTRAINT
                if dialect == Dialect::Sqlite {
                    return Err(MigrateError::MigrationError(format!(
                        "SQLite does not support ALTER TABLE DROP CHECK. \
                        To remove check constraint '{}' from table '{}', you need to recreate the table. \
                        Consider using a table rebuild migration.",
                        name, table
                    )));
                }

                Ok(match dialect {
                    // MySQL uses DROP CHECK
                    Dialect::Mysql => vec![format!("ALTER TABLE {} DROP CHECK {}", table, name)],
                    // PostgreSQL uses DROP CONSTRAINT
                    Dialect::Postgres => {
                        vec![format!("ALTER TABLE {} DROP CONSTRAINT {}", table, name)]
                    }
                    // SQLite case handled above
                    Dialect::Sqlite => unreachable!(),
                })
            }
        }
    }
}

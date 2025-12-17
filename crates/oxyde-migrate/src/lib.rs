//! Schema migration system with diff computation and SQL generation.
//!
//! This crate provides Django-style migrations for Oxyde ORM. It compares
//! model schemas (snapshots) and generates migration operations.
//!
//! # Architecture
//!
//! ```text
//! Models → Snapshot (JSON) → compute_diff() → MigrationOp[] → to_sql() → DDL
//! ```
//!
//! # Core Types
//!
//! ## Snapshot
//! Point-in-time representation of database schema:
//! - `tables`: HashMap of TableDef
//! - `version`: Schema version number
//!
//! ## TableDef
//! Table schema definition:
//! - `fields`: Column definitions (FieldDef)
//! - `indexes`: Index definitions (IndexDef)
//! - `foreign_keys`: FK constraints (ForeignKeyDef)
//! - `checks`: CHECK constraints (CheckDef)
//!
//! ## MigrationOp
//! Individual migration operation (enum):
//! - CreateTable, DropTable, RenameTable
//! - AddColumn, DropColumn, RenameColumn, AlterColumn
//! - CreateIndex, DropIndex
//! - AddForeignKey, DropForeignKey
//! - AddCheck, DropCheck
//!
//! # Dialect Support
//!
//! - **PostgreSQL**: Full ALTER TABLE support
//! - **SQLite**: Limited ALTER (requires table rebuild for some ops)
//! - **MySQL**: Full support with CHANGE/MODIFY syntax
//!
//! # SQLite Limitations
//!
//! SQLite doesn't support:
//! - ALTER TABLE ADD CONSTRAINT (FK/CHECK)
//! - ALTER COLUMN (type changes)
//!
//! Solution: Table rebuild migration (12-step process):
//! 1. PRAGMA foreign_keys=OFF
//! 2. CREATE TABLE _new_X with new schema
//! 3. INSERT INTO _new_X SELECT * FROM X
//! 4. DROP TABLE X
//! 5. ALTER TABLE _new_X RENAME TO X
//! 6. Recreate indexes
//! 7. PRAGMA foreign_keys=ON
//!
//! # Usage
//!
//! ```rust,ignore
//! // Compute diff between snapshots
//! let ops = compute_diff(&old_snapshot, &new_snapshot);
//!
//! // Generate SQL for PostgreSQL
//! let migration = Migration { name: "0001".into(), operations: ops };
//! let sql_statements = migration.to_sql(Dialect::Postgres)?;
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Supported database dialects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    Sqlite,
    Postgres,
    Mysql,
}

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("Migration error: {0}")]
    MigrationError(String),

    #[error("Snapshot error: {0}")]
    SnapshotError(String),

    #[error("Diff error: {0}")]
    DiffError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

pub type Result<T> = std::result::Result<T, MigrateError>;

/// Field definition in schema
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldDef {
    pub name: String,
    /// Python type name for cross-dialect type generation (e.g., "int", "str", "bytes")
    pub python_type: String,
    /// Explicit db_type from user (e.g., "JSONB", "VARCHAR(255)")
    #[serde(default)]
    pub db_type: Option<String>,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<String>,
    #[serde(default)]
    pub auto_increment: bool,
}

/// Index definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexDef {
    pub name: String,
    pub fields: Vec<String>,
    pub unique: bool,
    pub method: Option<String>,
}

/// Foreign key definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForeignKeyDef {
    pub name: String,
    pub columns: Vec<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_delete: Option<String>, // CASCADE, SET NULL, RESTRICT, NO ACTION
    pub on_update: Option<String>,
}

/// Check constraint definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckDef {
    pub name: String,
    pub expression: String,
}

/// Table definition in schema
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub indexes: Vec<IndexDef>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKeyDef>,
    #[serde(default)]
    pub checks: Vec<CheckDef>,
    pub comment: Option<String>,
}

/// Schema snapshot
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub version: u32,
    pub tables: HashMap<String, TableDef>,
}

impl Snapshot {
    /// Create a new empty snapshot
    pub fn new() -> Self {
        Self {
            version: 1,
            tables: HashMap::new(),
        }
    }

    /// Add a table to the snapshot
    pub fn add_table(&mut self, table: TableDef) {
        self.tables.insert(table.name.clone(), table);
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| MigrateError::SerializationError(e.to_string()))
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| MigrateError::SerializationError(e.to_string()))
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Migration operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MigrationOp {
    CreateTable {
        table: TableDef,
    },
    DropTable {
        name: String,
        /// Full table definition for reverse migration (optional for forward migration)
        #[serde(skip_serializing_if = "Option::is_none")]
        table: Option<TableDef>,
    },
    RenameTable {
        old_name: String,
        new_name: String,
    },
    AddColumn {
        table: String,
        field: FieldDef,
    },
    DropColumn {
        table: String,
        field: String,
        /// Full field definition for reverse migration (optional for forward migration)
        #[serde(skip_serializing_if = "Option::is_none")]
        field_def: Option<FieldDef>,
    },
    RenameColumn {
        table: String,
        old_name: String,
        new_name: String,
        /// Full field definition - required for MySQL CHANGE command
        #[serde(skip_serializing_if = "Option::is_none")]
        field_def: Option<FieldDef>,
    },
    AlterColumn {
        table: String,
        old_field: FieldDef,
        new_field: FieldDef,
        /// Full table schema for SQLite rebuild (optional)
        #[serde(skip_serializing_if = "Option::is_none")]
        table_fields: Option<Vec<FieldDef>>,
        /// Table indexes for SQLite rebuild (optional)
        #[serde(skip_serializing_if = "Option::is_none")]
        table_indexes: Option<Vec<IndexDef>>,
        /// Table foreign keys for SQLite rebuild (optional)
        #[serde(skip_serializing_if = "Option::is_none")]
        table_foreign_keys: Option<Vec<ForeignKeyDef>>,
        /// Table check constraints for SQLite rebuild (optional)
        #[serde(skip_serializing_if = "Option::is_none")]
        table_checks: Option<Vec<CheckDef>>,
    },
    CreateIndex {
        table: String,
        index: IndexDef,
    },
    DropIndex {
        table: String,
        index: String,
        /// Full index definition for reverse migration
        index_def: IndexDef,
    },
    AddForeignKey {
        table: String,
        fk: ForeignKeyDef,
    },
    DropForeignKey {
        table: String,
        name: String,
        /// Full foreign key definition for reverse migration
        fk_def: ForeignKeyDef,
    },
    AddCheck {
        table: String,
        check: CheckDef,
    },
    DropCheck {
        table: String,
        name: String,
        /// Full check definition for reverse migration
        check_def: CheckDef,
    },
}

/// Generate SQL type from Python type name for a given dialect.
///
/// This is used when db_type is not explicitly specified by the user.
fn python_type_to_sql(python_type: &str, dialect: Dialect, is_pk: bool) -> String {
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
fn translate_db_type(db_type: &str, dialect: Dialect) -> String {
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

/// Migration file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Migration {
    pub name: String,
    pub operations: Vec<MigrationOp>,
}

impl Migration {
    /// Create a new migration
    pub fn new(name: String) -> Self {
        Self {
            name,
            operations: Vec::new(),
        }
    }

    /// Add an operation
    pub fn add_operation(&mut self, op: MigrationOp) {
        self.operations.push(op);
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| MigrateError::SerializationError(e.to_string()))
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| MigrateError::SerializationError(e.to_string()))
    }

    /// Generate SQL statements for this migration
    /// Returns Err if any operation is not supported by the dialect
    pub fn to_sql(&self, dialect: Dialect) -> Result<Vec<String>> {
        let mut all_sql = Vec::new();
        for op in &self.operations {
            let sqls = op.to_sql(dialect)?;
            all_sql.extend(sqls);
        }
        Ok(all_sql)
    }
}

/// Compute diff between two snapshots
pub fn compute_diff(old: &Snapshot, new: &Snapshot) -> Vec<MigrationOp> {
    let mut ops = Vec::new();

    // Find new tables
    for (name, table) in &new.tables {
        if !old.tables.contains_key(name) {
            ops.push(MigrationOp::CreateTable {
                table: table.clone(),
            });
        }
    }

    // Find dropped tables
    for (name, old_table) in &old.tables {
        if !new.tables.contains_key(name) {
            ops.push(MigrationOp::DropTable {
                name: name.clone(),
                table: Some(old_table.clone()),
            });
        }
    }

    // Find modified tables
    for (name, new_table) in &new.tables {
        if let Some(old_table) = old.tables.get(name) {
            // Compare fields - find added columns
            for new_field in &new_table.fields {
                if !old_table.fields.iter().any(|f| f.name == new_field.name) {
                    ops.push(MigrationOp::AddColumn {
                        table: name.clone(),
                        field: new_field.clone(),
                    });
                }
            }

            // Find dropped columns
            for old_field in &old_table.fields {
                if !new_table.fields.iter().any(|f| f.name == old_field.name) {
                    ops.push(MigrationOp::DropColumn {
                        table: name.clone(),
                        field: old_field.name.clone(),
                        field_def: Some(old_field.clone()),
                    });
                }
            }

            // Find altered columns (same name, different definition)
            for new_field in &new_table.fields {
                if let Some(old_field) = old_table.fields.iter().find(|f| f.name == new_field.name)
                {
                    // Check if type changed using python_type or db_type
                    let type_changed = if old_field.python_type != new_field.python_type {
                        true
                    } else {
                        old_field.db_type != new_field.db_type
                    };

                    let nullable_changed = old_field.nullable != new_field.nullable;
                    let default_changed = old_field.default != new_field.default;
                    let unique_changed = old_field.unique != new_field.unique;

                    if type_changed || nullable_changed || default_changed || unique_changed {
                        ops.push(MigrationOp::AlterColumn {
                            table: name.clone(),
                            old_field: old_field.clone(),
                            new_field: new_field.clone(),
                            // Note: these will be filled by Python for SQLite migrations
                            table_fields: None,
                            table_indexes: None,
                            table_foreign_keys: None,
                            table_checks: None,
                        });
                    }
                }
            }

            // Find added indexes
            for new_idx in &new_table.indexes {
                if !old_table.indexes.iter().any(|idx| idx.name == new_idx.name) {
                    ops.push(MigrationOp::CreateIndex {
                        table: name.clone(),
                        index: new_idx.clone(),
                    });
                }
            }

            // Find dropped indexes
            for old_idx in &old_table.indexes {
                if !new_table.indexes.iter().any(|idx| idx.name == old_idx.name) {
                    ops.push(MigrationOp::DropIndex {
                        table: name.clone(),
                        index: old_idx.name.clone(),
                        index_def: old_idx.clone(),
                    });
                }
            }

            // Find added foreign keys
            for new_fk in &new_table.foreign_keys {
                if !old_table
                    .foreign_keys
                    .iter()
                    .any(|fk| fk.name == new_fk.name)
                {
                    ops.push(MigrationOp::AddForeignKey {
                        table: name.clone(),
                        fk: new_fk.clone(),
                    });
                }
            }

            // Find dropped foreign keys
            for old_fk in &old_table.foreign_keys {
                if !new_table
                    .foreign_keys
                    .iter()
                    .any(|fk| fk.name == old_fk.name)
                {
                    ops.push(MigrationOp::DropForeignKey {
                        table: name.clone(),
                        name: old_fk.name.clone(),
                        fk_def: old_fk.clone(),
                    });
                }
            }

            // Find added check constraints
            for new_check in &new_table.checks {
                if !old_table.checks.iter().any(|c| c.name == new_check.name) {
                    ops.push(MigrationOp::AddCheck {
                        table: name.clone(),
                        check: new_check.clone(),
                    });
                }
            }

            // Find dropped check constraints
            for old_check in &old_table.checks {
                if !new_table.checks.iter().any(|c| c.name == old_check.name) {
                    ops.push(MigrationOp::DropCheck {
                        table: name.clone(),
                        name: old_check.name.clone(),
                        check_def: old_check.clone(),
                    });
                }
            }
        }
    }

    ops
}

/// Unit tests for private helper functions.
/// Integration tests for public API are in tests/migration_tests.rs
#[cfg(test)]
mod tests {
    use super::*;

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
}

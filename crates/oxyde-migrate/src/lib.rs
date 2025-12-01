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
    pub field_type: String,
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
        /// Full table definition for reverse migration
        table: TableDef,
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
        /// Full field definition for reverse migration
        field_def: FieldDef,
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

/// Build full MySQL column definition from FieldDef
fn build_mysql_column_def(field: &FieldDef, dialect: Dialect) -> String {
    let mut col_def = format!("{} {}", field.name, field.field_type);

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

    // For MySQL CHANGE, we need to handle the case where name might differ
    // This function returns definition with the field's name
    let _ = dialect; // dialect parameter for future use
    col_def
}

/// Build SQLite column definition from FieldDef
fn build_sqlite_column_def(field: &FieldDef) -> String {
    let mut col_def = format!("{} {}", field.name, field.field_type);

    if field.primary_key {
        col_def.push_str(" PRIMARY KEY");
        if field.auto_increment {
            col_def.push_str(" AUTOINCREMENT");
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
            table_parts.push(build_sqlite_column_def(new_field));
        } else {
            table_parts.push(build_sqlite_column_def(field));
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
                let mut fields_sql = Vec::new();

                for field in &table.fields {
                    let mut field_sql = format!("{} {}", field.name, field.field_type);

                    if field.primary_key {
                        field_sql.push_str(" PRIMARY KEY");
                    }

                    // Handle auto_increment flag (set from Python based on type mapping logic)
                    if field.auto_increment {
                        match dialect {
                            Dialect::Sqlite => field_sql.push_str(" AUTOINCREMENT"),
                            Dialect::Mysql => field_sql.push_str(" AUTO_INCREMENT"),
                            // Postgres uses SERIAL/BIGSERIAL types, no separate keyword needed
                            Dialect::Postgres => {}
                        }
                    }

                    if !field.nullable {
                        field_sql.push_str(" NOT NULL");
                    }

                    if field.unique && !field.primary_key {
                        field_sql.push_str(" UNIQUE");
                    }

                    if let Some(default) = &field.default {
                        field_sql.push_str(&format!(" DEFAULT {}", default));
                    }

                    fields_sql.push(field_sql);
                }

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
                let mut field_sql = format!("{} {}", field.name, field.field_type);

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
                            let col_def = build_mysql_column_def(&renamed_field, dialect);
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

                        // Change type if different
                        if old_field.field_type != new_field.field_type {
                            stmts.push(format!(
                                "ALTER TABLE {} ALTER COLUMN {} TYPE {}",
                                table, new_field.name, new_field.field_type
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
                        let col_def = build_mysql_column_def(new_field, dialect);
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
                table: old_table.clone(),
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
                        field_def: old_field.clone(),
                    });
                }
            }

            // Find altered columns (same name, different definition)
            for new_field in &new_table.fields {
                if let Some(old_field) = old_table.fields.iter().find(|f| f.name == new_field.name)
                {
                    // Check if any relevant attribute changed
                    let type_changed = old_field.field_type != new_field.field_type;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_field(name: &str) -> FieldDef {
        FieldDef {
            name: name.to_string(),
            field_type: "text".into(),
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
                    field_type: "integer".into(),
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
                    field_type: "INTEGER".into(),
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    default: None,
                    auto_increment: true,
                },
                FieldDef {
                    name: "author_id".into(),
                    field_type: "INTEGER".into(),
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
                field_type: "INTEGER".into(),
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
            vec!["ALTER TABLE users ADD COLUMN name text NOT NULL".to_string()]
        );
    }

    #[test]
    fn test_dialect_specific_sql() {
        // Test SQLite AUTOINCREMENT
        let pk_field = FieldDef {
            name: "id".into(),
            field_type: "INTEGER".into(),
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
            field_type: "INTEGER".into(),
            nullable: true,
            primary_key: false,
            unique: false,
            default: None,
            auto_increment: false,
        };
        let new_field = FieldDef {
            name: "age".into(),
            field_type: "TEXT".into(), // type change
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
            field_type: "INTEGER".into(),
            nullable: true,
            primary_key: false,
            unique: false,
            default: None,
            auto_increment: false,
        };
        let new_field = FieldDef {
            name: "age".into(),
            field_type: "TEXT".into(), // type change
            nullable: false,           // nullable change
            primary_key: false,
            unique: false,
            default: None,
            auto_increment: false,
        };

        // Full table schema
        let table_fields = vec![
            FieldDef {
                name: "id".into(),
                field_type: "INTEGER".into(),
                nullable: false,
                primary_key: true,
                unique: false,
                default: None,
                auto_increment: true,
            },
            old_field.clone(),
            FieldDef {
                name: "name".into(),
                field_type: "TEXT".into(),
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
            field_type: "VARCHAR(255)".into(),
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
                    field_type: "INTEGER".into(),
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    default: None,
                    auto_increment: true,
                },
                FieldDef {
                    name: "email".into(),
                    field_type: "VARCHAR(100)".into(),
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
                    field_type: "INTEGER".into(),
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    default: None,
                    auto_increment: true,
                },
                FieldDef {
                    name: "email".into(),
                    field_type: "VARCHAR(255)".into(), // Changed type
                    nullable: true,                    // Changed nullable
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
                assert_eq!(old_field.field_type, "VARCHAR(100)");
                assert_eq!(new_field.field_type, "VARCHAR(255)");
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
            field_type: "TEXT".into(),
            nullable: false,
            primary_key: false,
            unique: false,
            default: None,
            auto_increment: false,
        };
        let new_field = FieldDef {
            name: "email".into(),
            field_type: "TEXT".into(),
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
}

//! Schema diff computation and Migration struct.

use crate::op::MigrationOp;
use crate::types::{Dialect, MigrateError, Result, Snapshot};
use serde::{Deserialize, Serialize};

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

    /// Generate SQL statements for this migration.
    ///
    /// CREATE/DROP/INDEX statements come first, ALTER TABLE statements last.
    /// This ensures referenced tables exist before FK constraints are added
    /// (PG/MySQL emit FK as separate ALTER TABLE, not inline in CREATE TABLE).
    pub fn to_sql(&self, dialect: Dialect) -> Result<Vec<String>> {
        let mut all_sql = Vec::new();
        for op in &self.operations {
            let sqls = op.to_sql(dialect)?;
            all_sql.extend(sqls);
        }
        all_sql.sort_by_key(|s| {
            if s.trim_start().starts_with("ALTER") {
                1
            } else {
                0
            }
        });
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
                    let constraints_changed = old_field.max_length != new_field.max_length
                        || old_field.max_digits != new_field.max_digits
                        || old_field.decimal_places != new_field.decimal_places;

                    if type_changed
                        || nullable_changed
                        || default_changed
                        || unique_changed
                        || constraints_changed
                    {
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
                        name: old_idx.name.clone(),
                        index_def: Some(old_idx.clone()),
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
                        fk_def: Some(old_fk.clone()),
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
                        check_def: Some(old_check.clone()),
                    });
                }
            }
        }
    }

    ops
}

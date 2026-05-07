//! Schema diff computation and Migration struct.

use std::collections::{HashMap, HashSet};

use crate::op::MigrationOp;
use crate::types::{Dialect, MigrateError, Result, Snapshot, TableDef};
use serde::{Deserialize, Serialize};

/// Topologically sort table names so that referenced tables come before
/// tables that reference them. External FK targets (not in `tables`) are
/// ignored. Ties at the same topological level are broken alphabetically
/// for deterministic output.
///
/// Returns `Err(MigrateError::DiffError)` if a FK cycle is detected — such
/// schemas cannot be expressed as a linear CREATE TABLE sequence and require
/// the user to break the cycle (e.g. with `nullable=True` + separate ADD FK).
fn topo_sort_table_names(tables: &HashMap<String, TableDef>) -> Result<Vec<String>> {
    // in_degree[t] = number of FKs from t to other tables in `tables`
    let mut in_degree: HashMap<&str, usize> = tables.keys().map(|k| (k.as_str(), 0usize)).collect();

    for (name, table) in tables {
        for fk in &table.foreign_keys {
            if fk.ref_table != *name && tables.contains_key(&fk.ref_table) {
                *in_degree.get_mut(name.as_str()).unwrap() += 1;
            }
        }
    }

    // Kahn's algorithm with alphabetic tie-break for deterministic order
    let mut ready: Vec<String> = in_degree
        .iter()
        .filter_map(|(k, &d)| if d == 0 { Some((*k).to_string()) } else { None })
        .collect();
    ready.sort();

    let mut result = Vec::with_capacity(tables.len());
    let mut visited: HashSet<String> = HashSet::new();

    while let Some(node) = ready.pop() {
        if !visited.insert(node.clone()) {
            continue;
        }
        result.push(node.clone());

        // Decrement in_degree of every table that FK-references `node`
        let mut newly_ready: Vec<String> = Vec::new();
        for (other_name, other_table) in tables {
            if visited.contains(other_name) {
                continue;
            }
            let count = other_table
                .foreign_keys
                .iter()
                .filter(|fk| fk.ref_table == node && fk.ref_table != *other_name)
                .count();
            if count == 0 {
                continue;
            }
            if let Some(d) = in_degree.get_mut(other_name.as_str()) {
                *d = d.saturating_sub(count);
                if *d == 0 {
                    newly_ready.push(other_name.clone());
                }
            }
        }
        // Sort descending so `ready.pop()` pulls alphabetically lowest first
        newly_ready.sort_by(|a, b| b.cmp(a));
        ready.extend(newly_ready);
    }

    if result.len() != tables.len() {
        let mut remaining: Vec<&str> = tables
            .keys()
            .filter(|k| !visited.contains(k.as_str()))
            .map(|k| k.as_str())
            .collect();
        remaining.sort();
        return Err(MigrateError::DiffError(format!(
            "cyclic foreign key dependency among tables: {}. \
             Break the cycle by making one FK nullable and adding it in a \
             separate migration step.",
            remaining.join(", ")
        )));
    }

    Ok(result)
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

/// Compute diff between two snapshots.
///
/// Returns `Err` only when the create- or drop-subset itself contains a
/// foreign-key cycle (i.e. the cycle blocks linear CREATE/DROP ordering).
/// Cycles among unchanged tables are irrelevant and pass through silently.
pub fn compute_diff(old: &Snapshot, new: &Snapshot) -> Result<Vec<MigrationOp>> {
    let mut ops = Vec::new();

    // Topo-sort only the subset of tables that are actually being created.
    // FKs from this subset to tables that already exist in `old` are not
    // edges in the create-ordering graph (the targets exist regardless of
    // when this migration runs), and `topo_sort_table_names` already
    // ignores refs to tables outside the input map.
    let new_to_create: HashMap<String, TableDef> = new
        .tables
        .iter()
        .filter(|(name, _)| !old.tables.contains_key(*name))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let new_order = topo_sort_table_names(&new_to_create)?;
    for name in &new_order {
        if let Some(table) = new_to_create.get(name) {
            ops.push(MigrationOp::CreateTable {
                table: table.clone(),
            });
        }
    }

    // Topo-sort only the subset of tables that are actually being dropped,
    // then emit in reverse so referencing tables go before referenced ones.
    let old_to_drop: HashMap<String, TableDef> = old
        .tables
        .iter()
        .filter(|(name, _)| !new.tables.contains_key(*name))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let old_order = topo_sort_table_names(&old_to_drop)?;
    for name in old_order.iter().rev() {
        if let Some(old_table) = old_to_drop.get(name) {
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

            // Find dropped and changed indexes
            for old_idx in &old_table.indexes {
                match new_table
                    .indexes
                    .iter()
                    .find(|idx| idx.name == old_idx.name)
                {
                    Some(new_idx) if !new_idx.semantically_eq(old_idx) => {
                        ops.push(MigrationOp::DropIndex {
                            table: name.clone(),
                            name: old_idx.name.clone(),
                            index_def: Some(old_idx.clone()),
                        });
                        ops.push(MigrationOp::CreateIndex {
                            table: name.clone(),
                            index: new_idx.clone(),
                        });
                    }
                    None => {
                        ops.push(MigrationOp::DropIndex {
                            table: name.clone(),
                            name: old_idx.name.clone(),
                            index_def: Some(old_idx.clone()),
                        });
                    }
                    _ => {}
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

    Ok(ops)
}

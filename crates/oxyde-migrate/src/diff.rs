//! Schema diff computation.

use std::collections::{HashMap, HashSet};

use crate::Result;
use oxyde_codec::{ColumnTypeSpec, EnumFieldRef, MigrateError, MigrationOp, Snapshot, TableDef};

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

fn collect_enum_defs(snapshot: &Snapshot) -> Result<HashMap<String, Vec<String>>> {
    let mut defs = HashMap::new();
    for table in snapshot.tables.values() {
        for field in &table.fields {
            collect_enum_def_from_spec(&field.column_type, &mut defs)?;
        }
    }
    Ok(defs)
}

fn collect_enum_def_from_spec(
    spec: &ColumnTypeSpec,
    defs: &mut HashMap<String, Vec<String>>,
) -> Result<()> {
    match spec {
        ColumnTypeSpec::Enum { name, values } => {
            if let Some(existing) = defs.get(name) {
                if existing != values {
                    return Err(MigrateError::DiffError(format!(
                        "enum type '{}' has conflicting value sets",
                        name
                    )));
                }
            } else {
                defs.insert(name.clone(), values.clone());
            }
        }
        ColumnTypeSpec::Array { item } => collect_enum_def_from_spec(item, defs)?,
        _ => {}
    }
    Ok(())
}

fn sorted_keys(map: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut keys = map.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    keys
}

fn enum_values_are_append_only(old_values: &[String], new_values: &[String]) -> bool {
    new_values.len() >= old_values.len() && &new_values[..old_values.len()] == old_values
}

fn column_type_requires_alter(old: &ColumnTypeSpec, new: &ColumnTypeSpec) -> bool {
    match (old, new) {
        (
            ColumnTypeSpec::Enum { name: old_name, .. },
            ColumnTypeSpec::Enum { name: new_name, .. },
        ) => old_name != new_name,
        (ColumnTypeSpec::Array { item: old_item }, ColumnTypeSpec::Array { item: new_item }) => {
            column_type_requires_alter(old_item, new_item)
        }
        _ => old != new,
    }
}

fn db_type_requires_alter(
    old_type: &ColumnTypeSpec,
    new_type: &ColumnTypeSpec,
    old_db_type: &Option<String>,
    new_db_type: &Option<String>,
) -> bool {
    if !column_type_requires_alter(old_type, new_type) && contains_enum_type(old_type) {
        return false;
    }
    old_db_type != new_db_type
}

fn contains_enum_type(spec: &ColumnTypeSpec) -> bool {
    match spec {
        ColumnTypeSpec::Enum { .. } => true,
        ColumnTypeSpec::Array { item } => contains_enum_type(item),
        _ => false,
    }
}

fn scalar_enum_name(spec: &ColumnTypeSpec) -> Option<&str> {
    match spec {
        ColumnTypeSpec::Enum { name, .. } => Some(name),
        _ => None,
    }
}

fn existing_scalar_enum_fields(
    old: &Snapshot,
    new: &Snapshot,
    enum_name: &str,
    values: &[String],
) -> Vec<EnumFieldRef> {
    let mut fields = Vec::new();
    let mut table_names = old
        .tables
        .keys()
        .filter(|name| new.tables.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    table_names.sort();

    for table_name in table_names {
        let old_table = &old.tables[&table_name];
        let new_table = &new.tables[&table_name];
        for old_field in &old_table.fields {
            if scalar_enum_name(&old_field.column_type) != Some(enum_name) {
                continue;
            }
            if let Some(new_field) = new_table
                .fields
                .iter()
                .find(|field| field.name == old_field.name)
                .filter(|field| scalar_enum_name(&field.column_type) == Some(enum_name))
            {
                let mut field = new_field.clone();
                if let ColumnTypeSpec::Enum {
                    values: field_values,
                    ..
                } = &mut field.column_type
                {
                    *field_values = values.to_vec();
                }
                fields.push(EnumFieldRef {
                    table: table_name.clone(),
                    field,
                });
            }
        }
    }

    fields
}

/// Compute diff between two snapshots.
///
/// Returns `Err` only when the create- or drop-subset itself contains a
/// foreign-key cycle (i.e. the cycle blocks linear CREATE/DROP ordering).
/// Cycles among unchanged tables are irrelevant and pass through silently.
pub fn compute_diff(old: &Snapshot, new: &Snapshot) -> Result<Vec<MigrationOp>> {
    let mut ops = Vec::new();
    let old_enums = collect_enum_defs(old)?;
    let new_enums = collect_enum_defs(new)?;

    for name in sorted_keys(&new_enums) {
        if !old_enums.contains_key(&name) {
            ops.push(MigrationOp::CreateEnumType {
                name: name.clone(),
                values: new_enums[&name].clone(),
            });
        }
    }

    for name in sorted_keys(&new_enums) {
        if let Some(old_values) = old_enums.get(&name) {
            let new_values = &new_enums[&name];
            if enum_values_are_append_only(old_values, new_values) {
                for (index, value) in new_values[old_values.len()..].iter().cloned().enumerate() {
                    let values = &new_values[..old_values.len() + index + 1];
                    ops.push(MigrationOp::AddEnumValue {
                        name: name.clone(),
                        value,
                        fields: existing_scalar_enum_fields(old, new, &name, values),
                    });
                }
            } else {
                ops.push(MigrationOp::AlterEnumType {
                    name: name.clone(),
                    old_values: old_values.clone(),
                    new_values: new_values.clone(),
                });
            }
        }
    }

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
                    // Check if type changed using column_type or db_type
                    let type_changed =
                        column_type_requires_alter(&old_field.column_type, &new_field.column_type)
                            || db_type_requires_alter(
                                &old_field.column_type,
                                &new_field.column_type,
                                &old_field.db_type,
                                &new_field.db_type,
                            );

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

            // Find dropped and changed check constraints
            for old_check in &old_table.checks {
                match new_table.checks.iter().find(|c| c.name == old_check.name) {
                    Some(new_check) if new_check.expression != old_check.expression => {
                        ops.push(MigrationOp::DropCheck {
                            table: name.clone(),
                            name: old_check.name.clone(),
                            check_def: Some(old_check.clone()),
                        });
                        ops.push(MigrationOp::AddCheck {
                            table: name.clone(),
                            check: new_check.clone(),
                        });
                    }
                    None => {
                        ops.push(MigrationOp::DropCheck {
                            table: name.clone(),
                            name: old_check.name.clone(),
                            check_def: Some(old_check.clone()),
                        });
                    }
                    _ => {}
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
        }
    }

    for name in sorted_keys(&old_enums) {
        if !new_enums.contains_key(&name) {
            ops.push(MigrationOp::DropEnumType {
                name: name.clone(),
                values: Some(old_enums[&name].clone()),
            });
        }
    }

    Ok(ops)
}

//! Schema diff computation.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::Result;
use oxyde_codec::{
    ColumnTypeSpec, EnumFieldRef, ForeignKeyDef, IndexDef, MigrateError, MigrationOp, Snapshot,
    TableDef,
};

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
            .map(std::string::String::as_str)
            .collect();
        remaining.sort_unstable();
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
                        "enum type '{name}' has conflicting value sets"
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
    old_db_type: Option<&str>,
    new_db_type: Option<&str>,
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

fn index_covers_columns(index: &IndexDef, columns: &[String]) -> bool {
    !columns.is_empty()
        && index.fields.len() >= columns.len()
        && index.fields[..columns.len()] == *columns
}

fn index_supports_fk(
    index_table: &str,
    index: &IndexDef,
    fk_table: &str,
    fk: &ForeignKeyDef,
) -> bool {
    (index_table == fk_table && index_covers_columns(index, &fk.columns))
        || (index_table == fk.ref_table && index_covers_columns(index, &fk.ref_columns))
}

fn added_or_altered_column(op: &MigrationOp) -> Option<(&str, &str)> {
    match op {
        MigrationOp::AddColumn { table, field } => Some((table, &field.name)),
        MigrationOp::AlterColumn {
            table, new_field, ..
        } => Some((table, &new_field.name)),
        _ => None,
    }
}

fn fk_uses_column(fk_table: &str, fk: &ForeignKeyDef, table: &str, column: &str) -> bool {
    (fk_table == table && fk.columns.iter().any(|name| name == column))
        || (fk.ref_table == table && fk.ref_columns.iter().any(|name| name == column))
}

fn add_dependency(prerequisites: &mut [BTreeSet<usize>], before: usize, after: usize) {
    if before != after {
        prerequisites[after].insert(before);
    }
}

fn add_column_drop_dependencies(
    prerequisites: &mut [BTreeSet<usize>],
    operations: &[MigrationOp],
    after: usize,
    table: &str,
    column: &str,
) {
    for (before, candidate) in operations.iter().enumerate() {
        match candidate {
            MigrationOp::DropForeignKey {
                table: fk_table,
                fk_def: Some(fk),
                ..
            } if fk_uses_column(fk_table, fk, table, column) => {
                add_dependency(prerequisites, before, after);
            }
            MigrationOp::DropIndex {
                table: index_table,
                index_def: Some(index),
                ..
            } if index_table == table && index.fields.iter().any(|name| name == column) => {
                add_dependency(prerequisites, before, after);
            }
            MigrationOp::DropCheck {
                table: check_table, ..
            } if check_table == table => {
                add_dependency(prerequisites, before, after);
            }
            MigrationOp::DropTable {
                table: Some(dropped_table),
                ..
            } if dropped_table
                .foreign_keys
                .iter()
                .any(|fk| fk_uses_column(&dropped_table.name, fk, table, column)) =>
            {
                add_dependency(prerequisites, before, after);
            }
            _ => {}
        }
    }
}

/// Stably order generated operations from their concrete schema dependencies.
///
/// `compute_diff` owns full table, index, and foreign-key definitions, so it can
/// safely move only prerequisites while leaving unrelated generated operations
/// in their deterministic discovery order. The SQL renderer intentionally does
/// not call this function: hand-authored migrations remain authored-order code.
fn order_generated_operations(operations: Vec<MigrationOp>) -> Result<Vec<MigrationOp>> {
    let mut prerequisites = vec![BTreeSet::new(); operations.len()];

    for (after, operation) in operations.iter().enumerate() {
        match operation {
            MigrationOp::CreateTable { table } => {
                for fk in &table.foreign_keys {
                    for (before, candidate) in operations.iter().enumerate() {
                        match candidate {
                            MigrationOp::CreateTable { table: referenced }
                                if referenced.name == fk.ref_table
                                    && referenced.name != table.name =>
                            {
                                add_dependency(&mut prerequisites, before, after);
                            }
                            MigrationOp::CreateIndex {
                                table: index_table,
                                index,
                            } if index_supports_fk(index_table, index, &table.name, fk) => {
                                add_dependency(&mut prerequisites, before, after);
                            }
                            _ => {
                                if let Some((column_table, column)) =
                                    added_or_altered_column(candidate)
                                {
                                    if column_table == fk.ref_table
                                        && fk.ref_columns.iter().any(|name| name == column)
                                    {
                                        add_dependency(&mut prerequisites, before, after);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            MigrationOp::CreateIndex { table, index } => {
                for (before, candidate) in operations.iter().enumerate() {
                    match candidate {
                        MigrationOp::CreateTable { table: created } if created.name == *table => {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        MigrationOp::DropIndex {
                            table: dropped_table,
                            name,
                            ..
                        } if dropped_table == table && name == &index.name => {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        _ => {
                            if let Some((column_table, column)) = added_or_altered_column(candidate)
                            {
                                if column_table == table
                                    && index.fields.iter().any(|name| name == column)
                                {
                                    add_dependency(&mut prerequisites, before, after);
                                }
                            }
                        }
                    }
                }
            }
            MigrationOp::AddForeignKey { table, fk } => {
                for (before, candidate) in operations.iter().enumerate() {
                    match candidate {
                        MigrationOp::CreateTable { table: created }
                            if created.name == *table || created.name == fk.ref_table =>
                        {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        MigrationOp::CreateIndex {
                            table: index_table,
                            index,
                        } if index_supports_fk(index_table, index, table, fk) => {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        MigrationOp::DropForeignKey {
                            table: dropped_table,
                            name,
                            ..
                        } if dropped_table == table && name == &fk.name => {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        _ => {
                            if let Some((column_table, column)) = added_or_altered_column(candidate)
                            {
                                if fk_uses_column(table, fk, column_table, column) {
                                    add_dependency(&mut prerequisites, before, after);
                                }
                            }
                        }
                    }
                }
            }
            MigrationOp::AddCheck { table, check } => {
                for (before, candidate) in operations.iter().enumerate() {
                    match candidate {
                        MigrationOp::CreateTable { table: created } if created.name == *table => {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        MigrationOp::DropCheck {
                            table: dropped_table,
                            name,
                            ..
                        } if dropped_table == table && name == &check.name => {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        _ => {
                            if let Some((column_table, _)) = added_or_altered_column(candidate) {
                                if column_table == table {
                                    add_dependency(&mut prerequisites, before, after);
                                }
                            }
                        }
                    }
                }
            }
            MigrationOp::DropTable { name, .. } => {
                for (before, candidate) in operations.iter().enumerate() {
                    match candidate {
                        MigrationOp::DropForeignKey {
                            table,
                            fk_def: Some(fk),
                            ..
                        } if table == name || fk.ref_table == *name => {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        MigrationOp::DropIndex { table, .. }
                        | MigrationOp::DropCheck { table, .. }
                            if table == name =>
                        {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        MigrationOp::DropTable {
                            name: child_name,
                            table: Some(child),
                        } if child_name != name
                            && child.foreign_keys.iter().any(|fk| fk.ref_table == *name) =>
                        {
                            add_dependency(&mut prerequisites, before, after);
                        }
                        _ => {}
                    }
                }
            }
            MigrationOp::DropIndex {
                table,
                index_def: Some(index),
                ..
            } => {
                for (before, candidate) in operations.iter().enumerate() {
                    if let MigrationOp::DropForeignKey {
                        table: fk_table,
                        fk_def: Some(fk),
                        ..
                    } = candidate
                    {
                        if index_supports_fk(table, index, fk_table, fk) {
                            add_dependency(&mut prerequisites, before, after);
                        }
                    }
                }
            }
            MigrationOp::DropColumn { table, field, .. } => {
                add_column_drop_dependencies(&mut prerequisites, &operations, after, table, field);
            }
            MigrationOp::AlterColumn {
                table, old_field, ..
            } => {
                add_column_drop_dependencies(
                    &mut prerequisites,
                    &operations,
                    after,
                    table,
                    &old_field.name,
                );
            }
            _ => {}
        }
    }

    // Stable Kahn ordering preserves the staged discovery order whenever more
    // than one operation is ready. A DFS post-order can hoist a later
    // prerequisite ahead of an unrelated earlier operation and cross the
    // dependency-drop / table-drop / column-change staging boundaries below.
    let mut dependents = vec![BTreeSet::new(); operations.len()];
    let mut in_degree = vec![0usize; operations.len()];
    for (after, required) in prerequisites.iter().enumerate() {
        in_degree[after] = required.len();
        for &before in required {
            dependents[before].insert(after);
        }
    }

    let mut ready = in_degree
        .iter()
        .enumerate()
        .filter_map(|(index, &degree)| (degree == 0).then_some(index))
        .collect::<BTreeSet<_>>();
    let mut ordered_indices = Vec::with_capacity(operations.len());
    while let Some(index) = ready.pop_first() {
        ordered_indices.push(index);
        for &dependent in &dependents[index] {
            in_degree[dependent] -= 1;
            if in_degree[dependent] == 0 {
                ready.insert(dependent);
            }
        }
    }

    if ordered_indices.len() != operations.len() {
        let index = in_degree
            .iter()
            .position(|&degree| degree != 0)
            .expect("an incomplete topological order must leave a dependency");
        return Err(MigrateError::DiffError(format!(
            "cyclic dependency while ordering generated migration operation: {:?}",
            operations[index]
        )));
    }

    let mut operation_slots = operations.into_iter().map(Some).collect::<Vec<_>>();
    Ok(ordered_indices
        .into_iter()
        .map(|index| operation_slots[index].take().unwrap())
        .collect())
}

/// Compute diff between two snapshots.
///
/// Returns `Err` when schema definitions conflict or when the generated
/// operations contain a dependency cycle that prevents a linear migration.
/// Foreign-key cycles wholly among unchanged tables remain irrelevant.
pub fn compute_diff(old: &Snapshot, new: &Snapshot) -> Result<Vec<MigrationOp>> {
    let mut ops = Vec::new();
    // The differ has both schema snapshots, so it can stage dependency
    // operations, including relationships that span tables, without changing
    // authored renderer order.
    let mut dependency_drops = Vec::new();
    let mut column_changes = Vec::new();
    let mut dependency_adds = Vec::new();
    let old_enums = collect_enum_defs(old)?;
    let new_enums = collect_enum_defs(new)?;

    // Generated migrations must create enum types before tables or columns can
    // reference them. `order_generated_operations` has no enum graph edges, so
    // this insertion position is an explicit ordering invariant.
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
                    let values = &new_values[..=(old_values.len() + index)];
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
    // Find modified tables (sorted: HashMap order must not leak into files)
    let mut modified: Vec<&String> = new.tables.keys().collect();
    modified.sort();
    for name in modified {
        let new_table = &new.tables[name];
        if let Some(old_table) = old.tables.get(name) {
            // Compare fields - find added columns
            for new_field in &new_table.fields {
                if !old_table.fields.iter().any(|f| f.name == new_field.name) {
                    column_changes.push(MigrationOp::AddColumn {
                        table: name.clone(),
                        field: new_field.clone(),
                    });
                }
            }

            // Find dropped columns
            for old_field in &old_table.fields {
                if !new_table.fields.iter().any(|f| f.name == old_field.name) {
                    column_changes.push(MigrationOp::DropColumn {
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
                                old_field.db_type.as_deref(),
                                new_field.db_type.as_deref(),
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
                        column_changes.push(MigrationOp::AlterColumn {
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
                        dependency_drops.push(MigrationOp::DropIndex {
                            table: name.clone(),
                            name: old_idx.name.clone(),
                            index_def: Some(old_idx.clone()),
                        });
                        dependency_adds.push(MigrationOp::CreateIndex {
                            table: name.clone(),
                            index: new_idx.clone(),
                        });
                    }
                    None => {
                        dependency_drops.push(MigrationOp::DropIndex {
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
                    dependency_adds.push(MigrationOp::CreateIndex {
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
                    dependency_adds.push(MigrationOp::AddForeignKey {
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
                    dependency_drops.push(MigrationOp::DropForeignKey {
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
                        dependency_drops.push(MigrationOp::DropCheck {
                            table: name.clone(),
                            name: old_check.name.clone(),
                            check_def: Some(old_check.clone()),
                        });
                        dependency_adds.push(MigrationOp::AddCheck {
                            table: name.clone(),
                            check: new_check.clone(),
                        });
                    }
                    None => {
                        dependency_drops.push(MigrationOp::DropCheck {
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
                    dependency_adds.push(MigrationOp::AddCheck {
                        table: name.clone(),
                        check: new_check.clone(),
                    });
                }
            }
        }
    }

    // Remove dependencies before dropping columns or tables.
    ops.extend(dependency_drops);
    for name in old_order.iter().rev() {
        if let Some(old_table) = old_to_drop.get(name) {
            ops.push(MigrationOp::DropTable {
                name: name.clone(),
                table: Some(old_table.clone()),
            });
        }
    }
    // Preserve the existing Add/Drop/Alter column order while ensuring every
    // table's new columns precede indexes and constraints that use them.
    ops.extend(column_changes);
    ops.extend(dependency_adds);

    // Generated migrations must remove dependent tables and columns before
    // dropping their enum types. As above, the insertion position deliberately
    // carries this invariant instead of enum-specific graph edges.
    for name in sorted_keys(&old_enums) {
        if !new_enums.contains_key(&name) {
            ops.push(MigrationOp::DropEnumType {
                name: name.clone(),
                values: Some(old_enums[&name].clone()),
            });
        }
    }

    order_generated_operations(ops)
}

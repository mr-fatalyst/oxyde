//! DDL generation using sea-query builders + migration statement ordering.
//!
//! Type mapping is spec-driven (`spec_sql::resolve_spec_type`): semantic kind
//! from `ColumnTypeSpec`, verbatim user DDL from `FieldDef.db_type`. DDL
//! structure (CREATE/ALTER/DROP TABLE, indexes, foreign keys) uses sea-query
//! for dialect-specific syntax and identifier quoting.

use std::collections::BTreeSet;

use sea_query::{
    Alias, ColumnDef as SeaColumnDef, Expr, ForeignKey as SeaForeignKey,
    ForeignKeyAction as SeaFkAction, Index as SeaIndex, IndexType, IntoIden, MysqlQueryBuilder,
    PostgresQueryBuilder, SqliteQueryBuilder, Table as SeaTable,
};
use serde::{Deserialize, Serialize};

use crate::Dialect;
use oxyde_codec::{
    CheckDef, ColumnTypeSpec, FieldDef, ForeignKeyDef, IndexDef, MigrateError, MigrationOp,
};

type Result<T> = std::result::Result<T, MigrateError>;

/// Migration file: a named list of operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Migration {
    pub name: String,
    pub operations: Vec<MigrationOp>,
}

fn operation_table(op: &MigrationOp) -> Option<&str> {
    match op {
        MigrationOp::CreateTable { table } => Some(&table.name),
        MigrationOp::DropTable { name, .. } => Some(name),
        MigrationOp::AddColumn { table, .. }
        | MigrationOp::DropColumn { table, .. }
        | MigrationOp::RenameColumn { table, .. }
        | MigrationOp::AlterColumn { table, .. }
        | MigrationOp::CreateIndex { table, .. }
        | MigrationOp::DropIndex { table, .. }
        | MigrationOp::AddForeignKey { table, .. }
        | MigrationOp::DropForeignKey { table, .. }
        | MigrationOp::AddCheck { table, .. }
        | MigrationOp::DropCheck { table, .. } => Some(table),
        MigrationOp::CreateEnumType { .. }
        | MigrationOp::DropEnumType { .. }
        | MigrationOp::AddEnumValue { .. }
        | MigrationOp::AlterEnumType { .. }
        | MigrationOp::RenameTable { .. } => None,
    }
}

fn requires_created_table(op: &MigrationOp) -> bool {
    matches!(
        op,
        MigrationOp::AddColumn { .. }
            | MigrationOp::AlterColumn { .. }
            | MigrationOp::RenameColumn { .. }
            | MigrationOp::CreateIndex { .. }
            | MigrationOp::AddForeignKey { .. }
            | MigrationOp::AddCheck { .. }
    )
}

fn is_constraint_drop(op: &MigrationOp) -> bool {
    matches!(
        op,
        MigrationOp::DropForeignKey { .. }
            | MigrationOp::DropCheck { .. }
            | MigrationOp::DropIndex { .. }
    )
}

fn is_destructive_table_change(op: &MigrationOp) -> bool {
    matches!(
        op,
        MigrationOp::DropColumn { .. } | MigrationOp::DropTable { .. }
    )
}

/// Whether a create separates two operations that use the same table name.
/// Such a boundary means the operations may belong to different table lifetimes.
fn has_table_create_between(
    operations: &[MigrationOp],
    table: &str,
    first: usize,
    second: usize,
) -> bool {
    let (start, end) = if first < second {
        (first, second)
    } else {
        (second, first)
    };
    if start + 1 >= end {
        return false;
    }
    operations[start + 1..end].iter().any(
        |op| matches!(op, MigrationOp::CreateTable { table: created } if created.name == table),
    )
}

/// Whether a drop separates two operations that use the same table name.
/// Such a boundary means the operations may belong to different table lifetimes.
fn has_table_drop_between(
    operations: &[MigrationOp],
    table: &str,
    first: usize,
    second: usize,
) -> bool {
    let (start, end) = if first < second {
        (first, second)
    } else {
        (second, first)
    };
    if start + 1 >= end {
        return false;
    }
    operations[start + 1..end]
        .iter()
        .any(|op| matches!(op, MigrationOp::DropTable { name, .. } if name == table))
}

fn contains_enum(spec: &ColumnTypeSpec, enum_name: &str) -> bool {
    match spec {
        ColumnTypeSpec::Enum { name, .. } => name == enum_name,
        ColumnTypeSpec::Array { item } => contains_enum(item, enum_name),
        _ => false,
    }
}

fn operation_uses_enum(op: &MigrationOp, enum_name: &str) -> bool {
    match op {
        MigrationOp::CreateTable { table } => table
            .fields
            .iter()
            .any(|field| contains_enum(&field.column_type, enum_name)),
        MigrationOp::AddColumn { field, .. } => contains_enum(&field.column_type, enum_name),
        MigrationOp::AlterColumn { new_field, .. } => {
            contains_enum(&new_field.column_type, enum_name)
        }
        MigrationOp::AddEnumValue { name, .. } => name == enum_name,
        _ => false,
    }
}

fn operation_may_release_enum(op: &MigrationOp, enum_name: &str) -> bool {
    match op {
        MigrationOp::DropTable {
            table: Some(table), ..
        } => table
            .fields
            .iter()
            .any(|field| contains_enum(&field.column_type, enum_name)),
        MigrationOp::DropTable { table: None, .. } => true,
        MigrationOp::DropColumn {
            field_def: Some(field),
            ..
        } => contains_enum(&field.column_type, enum_name),
        MigrationOp::DropColumn {
            field_def: None, ..
        } => true,
        MigrationOp::AlterColumn {
            old_field,
            new_field,
            ..
        } => {
            contains_enum(&old_field.column_type, enum_name)
                && !contains_enum(&new_field.column_type, enum_name)
        }
        _ => false,
    }
}

fn add_enum_value_targets_table(op: &MigrationOp, table: &str) -> bool {
    matches!(
        op,
        MigrationOp::AddEnumValue { fields, .. }
            if fields.iter().any(|field| field.table == table)
    )
}

fn add_dependency(
    outgoing: &mut [BTreeSet<usize>],
    in_degree: &mut [usize],
    before: usize,
    after: usize,
) {
    if before != after && outgoing[before].insert(after) {
        in_degree[after] += 1;
    }
}

/// Return a stable topological order for migration operations.
///
/// Authored order is the tie-breaker between dependency-valid operations.
/// Dependencies encode concrete schema relationships instead of globally
/// grouping operations by their rendered SQL type. This also covers dialects
/// that emit relationships such as foreign keys as separate `ALTER TABLE`
/// statements instead of including them in `CREATE TABLE`.
fn order_operations(operations: &[MigrationOp]) -> Result<Vec<usize>> {
    let mut outgoing = vec![BTreeSet::new(); operations.len()];
    let mut in_degree = vec![0usize; operations.len()];

    for (create_index, create_op) in operations.iter().enumerate() {
        let MigrationOp::CreateTable { table } = create_op else {
            continue;
        };

        for (op_index, op) in operations.iter().enumerate() {
            if requires_created_table(op)
                && operation_table(op) == Some(table.name.as_str())
                && !has_table_drop_between(operations, &table.name, create_index, op_index)
            {
                add_dependency(&mut outgoing, &mut in_degree, create_index, op_index);
            }

            if let MigrationOp::AddForeignKey { fk, .. } = op {
                if fk.ref_table == table.name
                    && !has_table_drop_between(operations, &table.name, create_index, op_index)
                {
                    add_dependency(&mut outgoing, &mut in_degree, create_index, op_index);
                }
            }

            if let MigrationOp::AddEnumValue { fields, .. } = op {
                if fields.iter().any(|field| field.table == table.name)
                    && !has_table_drop_between(operations, &table.name, create_index, op_index)
                {
                    add_dependency(&mut outgoing, &mut in_degree, create_index, op_index);
                }
            }

            if let MigrationOp::DropTable { name, .. } = op {
                if name == &table.name {
                    if op_index < create_index {
                        add_dependency(&mut outgoing, &mut in_degree, op_index, create_index);
                    } else {
                        add_dependency(&mut outgoing, &mut in_degree, create_index, op_index);
                    }
                }
            }
        }
    }

    for (drop_index, drop_op) in operations.iter().enumerate() {
        if !is_constraint_drop(drop_op) {
            continue;
        }
        let Some(table) = operation_table(drop_op) else {
            continue;
        };

        for (destructive_index, destructive_op) in operations.iter().enumerate() {
            if is_destructive_table_change(destructive_op)
                && operation_table(destructive_op) == Some(table)
                && !has_table_create_between(operations, table, drop_index, destructive_index)
            {
                add_dependency(&mut outgoing, &mut in_degree, drop_index, destructive_index);
            }
        }
    }

    for (column_index, column_op) in operations.iter().enumerate() {
        let MigrationOp::DropColumn { table, .. } = column_op else {
            continue;
        };
        for (table_index, table_op) in operations.iter().enumerate() {
            if matches!(table_op, MigrationOp::DropTable { name, .. } if name == table)
                && !has_table_create_between(operations, table, column_index, table_index)
            {
                add_dependency(&mut outgoing, &mut in_degree, column_index, table_index);
            }
        }
    }

    for (rename_index, rename_op) in operations.iter().enumerate() {
        let MigrationOp::RenameTable { old_name, new_name } = rename_op else {
            continue;
        };
        for (op_index, op) in operations.iter().enumerate() {
            let targets_old_name =
                operation_table(op) == Some(old_name) || add_enum_value_targets_table(op, old_name);
            let targets_new_name =
                operation_table(op) == Some(new_name) || add_enum_value_targets_table(op, new_name);
            if op_index < rename_index && targets_old_name {
                add_dependency(&mut outgoing, &mut in_degree, op_index, rename_index);
            } else if op_index > rename_index && targets_new_name {
                add_dependency(&mut outgoing, &mut in_degree, rename_index, op_index);
            }
        }
    }

    for (enum_index, enum_op) in operations.iter().enumerate() {
        match enum_op {
            MigrationOp::CreateEnumType { name, .. } => {
                for (op_index, op) in operations.iter().enumerate() {
                    if operation_uses_enum(op, name) {
                        add_dependency(&mut outgoing, &mut in_degree, enum_index, op_index);
                    }
                }
            }
            MigrationOp::DropEnumType { name, .. } => {
                for (op_index, op) in operations.iter().enumerate() {
                    if operation_may_release_enum(op, name) {
                        add_dependency(&mut outgoing, &mut in_degree, op_index, enum_index);
                    }
                }
            }
            MigrationOp::AddEnumValue { fields, .. } => {
                for (op_index, op) in operations.iter().enumerate() {
                    let depends_on_value = if fields.is_empty() {
                        !matches!(
                            op,
                            MigrationOp::CreateEnumType { .. }
                                | MigrationOp::DropEnumType { .. }
                                | MigrationOp::AddEnumValue { .. }
                                | MigrationOp::AlterEnumType { .. }
                        )
                    } else {
                        operation_table(op).is_some_and(|table| {
                            fields.iter().any(|field| field.table == table)
                                && matches!(
                                    op,
                                    MigrationOp::AddColumn { .. }
                                        | MigrationOp::AlterColumn { .. }
                                        | MigrationOp::CreateIndex { .. }
                                        | MigrationOp::AddForeignKey { .. }
                                        | MigrationOp::AddCheck { .. }
                                )
                        })
                    };
                    if depends_on_value {
                        add_dependency(&mut outgoing, &mut in_degree, enum_index, op_index);
                    }
                }
            }
            _ => {}
        }
    }

    let mut ready: BTreeSet<usize> = in_degree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect();
    let mut ordered = Vec::with_capacity(operations.len());

    while let Some(index) = ready.pop_first() {
        ordered.push(index);
        for dependent in &outgoing[index] {
            in_degree[*dependent] -= 1;
            if in_degree[*dependent] == 0 {
                ready.insert(*dependent);
            }
        }
    }

    if ordered.len() != operations.len() {
        let blocked = in_degree
            .iter()
            .enumerate()
            .filter_map(|(index, degree)| (*degree > 0).then_some(index.to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(MigrateError::MigrationError(format!(
            "cyclic migration operation dependencies at indices: {blocked}"
        )));
    }

    Ok(ordered)
}

impl Migration {
    /// Generate SQL statements for this migration.
    ///
    /// Operations are stably ordered by their concrete schema dependencies,
    /// with authored order used to break ties. Statements emitted by one
    /// operation remain contiguous and in renderer order.
    pub fn to_sql(&self, dialect: Dialect) -> Result<Vec<String>> {
        let mut all_sql = Vec::new();
        for index in order_operations(&self.operations)? {
            all_sql.extend(self.operations[index].to_sql(dialect)?);
        }
        Ok(all_sql)
    }
}

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

// ── sea-query helpers ───────────────────────────────────────────────────────

/// Convert `FieldDef` to sea-query `ColumnDef` with dialect-appropriate type and constraints.
fn field_to_column_def(field: &FieldDef, dialect: Dialect) -> SeaColumnDef {
    let sql_type = crate::spec_sql::resolve_spec_type(
        &field.column_type,
        field.db_type.as_deref(),
        dialect,
        field.primary_key,
    );
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
    let sql_type = crate::spec_sql::resolve_spec_type(
        &field.column_type,
        field.db_type.as_deref(),
        Dialect::Mysql,
        field.primary_key,
    );
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
        use std::fmt::Write as _;
        let _ = write!(col_def, " DEFAULT {default}");
    }

    col_def
}

/// Build CREATE INDEX SQL for an index on a table.
fn build_create_index(table: &str, index: &IndexDef, dialect: Dialect) -> Result<String> {
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

    let mut sql = build_sql!(stmt, dialect);

    if let Some(predicate) = index.normalized_where_clause() {
        if dialect == Dialect::Mysql {
            return Err(MigrateError::MigrationError(
                "MySQL does not support partial indexes with WHERE predicates".to_string(),
            ));
        }
        sql.push_str(" WHERE ");
        sql.push_str(predicate);
    }

    Ok(sql)
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
    let temp_table = format!("_new_{table}");

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
        stmts.push(build_create_index(table, index, Dialect::Sqlite)?);
    }

    stmts.push("PRAGMA foreign_keys=ON".to_string());

    Ok(stmts)
}

// ── MigrationOp::to_sql ────────────────────────────────────────────────────

/// SQL rendering for `MigrationOp` (the type itself lives in oxyde-codec).
pub trait MigrationOpExt {
    fn to_sql(&self, dialect: Dialect) -> Result<Vec<String>>;
}

impl MigrationOpExt for MigrationOp {
    /// Generate SQL for this migration operation.
    ///
    /// Returns `Err` for operations not supported by the dialect
    /// (e.g., ALTER COLUMN on SQLite without table schema).
    fn to_sql(&self, dialect: Dialect) -> Result<Vec<String>> {
        match self {
            MigrationOp::CreateEnumType { name, values } => {
                if dialect != Dialect::Postgres {
                    return Ok(Vec::new());
                }
                let labels = values
                    .iter()
                    .map(|value| crate::spec_sql::quote_sql_string(value))
                    .collect::<Vec<_>>()
                    .join(", ");
                Ok(vec![format!(
                    "CREATE TYPE {} AS ENUM ({})",
                    crate::utils::bind::quote_pg_type_path(name),
                    labels
                )])
            }

            MigrationOp::DropEnumType { name, values: _ } => {
                if dialect != Dialect::Postgres {
                    return Ok(Vec::new());
                }
                Ok(vec![format!(
                    "DROP TYPE {}",
                    crate::utils::bind::quote_pg_type_path(name)
                )])
            }

            MigrationOp::AddEnumValue {
                name,
                value,
                fields,
            } => {
                if dialect == Dialect::Mysql {
                    return Ok(fields
                        .iter()
                        .map(|field| {
                            format!(
                                "ALTER TABLE `{}` MODIFY COLUMN {}",
                                field.table,
                                mysql_column_def(&field.field)
                            )
                        })
                        .collect());
                }
                if dialect == Dialect::Sqlite {
                    return Ok(Vec::new());
                }
                Ok(vec![format!(
                    "ALTER TYPE {} ADD VALUE IF NOT EXISTS {}",
                    crate::utils::bind::quote_pg_type_path(name),
                    crate::spec_sql::quote_sql_string(value)
                )])
            }

            MigrationOp::AlterEnumType { .. } => Ok(Vec::new()),

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
                    sql.push(build_create_index(&table.name, index, dialect)?);
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
                            renamed.name.clone_from(new_name);
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
                    let old_sql_type = crate::spec_sql::resolve_spec_type(
                        &old_field.column_type,
                        old_field.db_type.as_deref(),
                        dialect,
                        old_field.primary_key,
                    );
                    let new_sql_type = crate::spec_sql::resolve_spec_type(
                        &new_field.column_type,
                        new_field.db_type.as_deref(),
                        dialect,
                        new_field.primary_key,
                    );

                    if old_sql_type != new_sql_type {
                        // ::text bridge — universal cast path (PG has no implicit text → enum).
                        stmts.push(format!(
                            "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" TYPE {} USING \"{}\"::text::{}",
                            table, new_field.name, new_sql_type, new_field.name, new_sql_type
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
                Ok(vec![build_create_index(table, index, dialect)?])
            }

            MigrationOp::DropIndex {
                table,
                name,
                index_def: _,
            } => {
                let mut stmt = SeaIndex::drop();
                stmt.name(name).table(Alias::new(table));
                Ok(vec![build_sql!(stmt, dialect)])
            }

            MigrationOp::AddForeignKey { table, fk } => {
                if dialect == Dialect::Sqlite {
                    return Err(MigrateError::MigrationError(format!(
                        "SQLite does not support ALTER TABLE ADD FOREIGN KEY. \
                        To add a foreign key to table '{table}', you need to recreate the table. \
                        Consider using a table rebuild migration."
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
                        To remove foreign key '{name}' from table '{table}', you need to recreate the table. \
                        Consider using a table rebuild migration."
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
                        To add a check constraint to table '{table}', you need to recreate the table. \
                        Consider using a table rebuild migration."
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
                        To remove check constraint '{name}' from table '{table}', you need to recreate the table. \
                        Consider using a table rebuild migration."
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

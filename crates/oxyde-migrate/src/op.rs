//! Migration operations enum.

use crate::types::{CheckDef, FieldDef, ForeignKeyDef, IndexDef, TableDef};
use serde::{Deserialize, Serialize};

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
        name: String,
        /// Full index definition for reverse migration (optional for forward migration)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index_def: Option<IndexDef>,
    },
    AddForeignKey {
        table: String,
        fk: ForeignKeyDef,
    },
    DropForeignKey {
        table: String,
        name: String,
        /// Full foreign key definition for reverse migration (optional for forward migration)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fk_def: Option<ForeignKeyDef>,
    },
    AddCheck {
        table: String,
        check: CheckDef,
    },
    DropCheck {
        table: String,
        name: String,
        /// Full check definition for reverse migration (optional for forward migration)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        check_def: Option<CheckDef>,
    },
}

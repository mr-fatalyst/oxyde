//! Core data types for the migration system.

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
    /// Max length for str fields (used for VARCHAR(N) on PG/MySQL)
    #[serde(default)]
    pub max_length: Option<u32>,
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

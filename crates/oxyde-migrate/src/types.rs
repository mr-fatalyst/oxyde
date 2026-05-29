//! Core data types for the migration system.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
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

fn normalize_optional_sql_fragment(value: Option<String>) -> Option<String> {
    value
        .map(|fragment| fragment.trim().to_string())
        .filter(|fragment| !fragment.is_empty())
}

fn is_none_or_blank(value: &Option<String>) -> bool {
    value
        .as_deref()
        .map(str::trim)
        .map_or(true, |fragment| fragment.is_empty())
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn serialize_normalized_optional_sql_fragment<S>(
    value: &Option<String>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value
        .as_deref()
        .map(str::trim)
        .filter(|fragment| !fragment.is_empty())
    {
        Some(fragment) => serializer.serialize_some(fragment),
        None => serializer.serialize_none(),
    }
}

fn deserialize_normalized_optional_sql_fragment<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(normalize_optional_sql_fragment(value))
}

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
    /// Total digits for decimal fields (used for DECIMAL(M,D))
    #[serde(default)]
    pub max_digits: Option<u32>,
    /// Decimal places for decimal fields (used for DECIMAL(M,D))
    #[serde(default)]
    pub decimal_places: Option<u32>,
}

/// Index definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexDef {
    pub name: String,
    pub fields: Vec<String>,
    pub unique: bool,
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub nulls_not_distinct: bool,
    #[serde(
        default,
        rename = "where",
        skip_serializing_if = "is_none_or_blank",
        serialize_with = "serialize_normalized_optional_sql_fragment",
        deserialize_with = "deserialize_normalized_optional_sql_fragment"
    )]
    pub where_clause: Option<String>,
}

impl IndexDef {
    pub fn normalized_where_clause(&self) -> Option<&str> {
        self.where_clause
            .as_deref()
            .map(str::trim)
            .filter(|fragment| !fragment.is_empty())
    }

    pub fn semantically_eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.fields == other.fields
            && self.unique == other.unique
            && self.method == other.method
            && self.nulls_not_distinct == other.nulls_not_distinct
            && self.normalized_where_clause() == other.normalized_where_clause()
    }
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

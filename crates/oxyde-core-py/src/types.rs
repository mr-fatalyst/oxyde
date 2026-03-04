//! Result types for mutation operations (msgpack serializable).

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Result of an INSERT operation (msgpack serializable)
#[derive(Serialize, Deserialize)]
pub(crate) struct InsertResult {
    pub(crate) affected: usize,
    pub(crate) inserted_ids: Vec<JsonValue>,
}

/// Result of an UPDATE or DELETE operation (msgpack serializable)
#[derive(Serialize, Deserialize)]
pub(crate) struct MutationResult {
    pub(crate) affected: u64,
}

/// Result of an UPDATE or DELETE operation with RETURNING clause (msgpack serializable)
/// Uses columnar format: (columns, rows) for memory efficiency
#[derive(Serialize, Deserialize)]
pub(crate) struct MutationWithReturningResult {
    pub(crate) affected: usize,
    pub(crate) columns: Vec<String>,
    pub(crate) rows: Vec<Vec<JsonValue>>,
}

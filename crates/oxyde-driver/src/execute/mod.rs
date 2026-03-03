//! Query execution: traits, pool/transaction paths, INSERT RETURNING.

pub mod insert;
pub mod query;
pub mod traits;

// Traits used by query.rs via `use crate::execute::traits::{ConnExec, PoolExec}`

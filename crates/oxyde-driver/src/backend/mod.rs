//! Database backend test utilities
//!
//! This module contains tests for EXPLAIN query generation across different backends.
//! The actual backend implementations are in the `bind`, `convert`, and `explain` modules.

pub mod mysql;
pub mod postgres;
pub mod sqlite;

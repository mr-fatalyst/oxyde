//! PyO3 Python extension module exposing Rust core to Python.
//!
//! This crate builds the `_oxyde_core` Python module that provides the bridge
//! between Python's ORM layer and Rust's database execution layer.
//!
//! # Module Name
//!
//! Compiled as `_oxyde_core.cpython-*.so` and imported as:
//! ```python
//! import _oxyde_core
//! ```
//!
//! # Architecture
//!
//! ```text
//! Python Query → IR dict → msgpack → Rust → SQL → DB
//!                                         ↓
//! Python models ← Pydantic ← msgpack ← rows
//! ```
//!
//! # Exposed Functions
//!
//! ## Pool Management
//! - `init_pool(name, url, settings)` → Coroutine
//! - `init_pool_overwrite(name, url, settings)` → Coroutine
//! - `close_pool(name)` → Coroutine
//! - `close_all_pools()` → Coroutine
//!
//! ## Query Execution
//! - `execute(pool_name, ir_bytes)` → Coroutine[bytes]
//! - `execute_in_transaction(pool_name, tx_id, ir_bytes)` → Coroutine[bytes]
//!
//! ## Transactions
//! - `begin_transaction(pool_name)` → Coroutine[int]
//! - `commit_transaction(tx_id)` → Coroutine
//! - `rollback_transaction(tx_id)` → Coroutine
//! - `create_savepoint(tx_id, name)` → Coroutine
//! - `rollback_to_savepoint(tx_id, name)` → Coroutine
//! - `release_savepoint(tx_id, name)` → Coroutine
//!
//! ## Debug/Introspection
//! - `render_sql(pool_name, ir_bytes)` → Coroutine[(str, list)]
//! - `render_sql_debug(ir_bytes, dialect)` → (str, list)
//! - `explain(pool_name, ir_bytes, analyze, format)` → Coroutine
//!
//! ## Migrations
//! - `migration_compute_diff(old_json, new_json)` → str
//! - `migration_to_sql(operations_json, dialect)` → list[str]
//!
//! # Async Integration
//!
//! Uses `pyo3_asyncio::tokio` to expose Rust async functions as Python coroutines.
//! All async functions return awaitable objects compatible with asyncio.
//!
//! # Validation Strategy
//!
//! - **Write path**: Pydantic validates in Python before Rust receives data
//! - **Read path**: Rust returns raw data, Python validates with Pydantic
//! - **Rust layer**: Only validates IR structure, not data values
//!
//! # ABI Version
//!
//! `__abi_version__ = 1` exposed for Python-side compatibility checking.

// Use mimalloc as global allocator if feature enabled
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Use jemalloc as global allocator if feature enabled
#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod convert;
mod execute;
mod migration;
mod pool;
mod types;

use pyo3::prelude::*;

/// ABI version for compatibility checking
const ABI_VERSION: u32 = 1;

/// Python module definition
#[pymodule(gil_used = false)]
fn _oxyde_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__abi_version__", ABI_VERSION)?;

    m.add_function(wrap_pyfunction!(pool::init_pool, m)?)?;
    m.add_function(wrap_pyfunction!(pool::init_pool_overwrite, m)?)?;
    m.add_function(wrap_pyfunction!(pool::close_pool, m)?)?;
    m.add_function(wrap_pyfunction!(pool::close_all_pools, m)?)?;
    m.add_function(wrap_pyfunction!(execute::execute, m)?)?;
    m.add_function(wrap_pyfunction!(pool::begin_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(pool::commit_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(pool::rollback_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(pool::create_savepoint, m)?)?;
    m.add_function(wrap_pyfunction!(pool::rollback_to_savepoint, m)?)?;
    m.add_function(wrap_pyfunction!(pool::release_savepoint, m)?)?;
    m.add_function(wrap_pyfunction!(execute::execute_in_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(execute::render_sql, m)?)?;
    m.add_function(wrap_pyfunction!(execute::render_sql_debug, m)?)?;
    m.add_function(wrap_pyfunction!(execute::explain, m)?)?;

    // Migration functions
    m.add_function(wrap_pyfunction!(migration::migration_compute_diff, m)?)?;
    m.add_function(wrap_pyfunction!(migration::migration_to_sql, m)?)?;

    Ok(())
}

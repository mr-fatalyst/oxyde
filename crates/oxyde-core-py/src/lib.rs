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

use std::time::{Duration, Instant};

use oxyde_codec::QueryIR;
use oxyde_driver::{
    begin_transaction as driver_begin_transaction, close_all_pools as driver_close_all_pools,
    close_pool as driver_close_pool, commit_transaction as driver_commit_transaction,
    create_savepoint as driver_create_savepoint, execute_insert_returning,
    execute_insert_returning_in_transaction, execute_query_columnar,
    execute_query_columnar_in_transaction, execute_statement, execute_statement_in_transaction,
    explain_query, init_pool as driver_init_pool,
    init_pool_overwrite as driver_init_pool_overwrite, pool_backend as driver_pool_backend,
    release_savepoint as driver_release_savepoint,
    rollback_to_savepoint as driver_rollback_to_savepoint,
    rollback_transaction as driver_rollback_transaction, DatabaseBackend, ExplainFormat,
    ExplainOptions, PoolSettings as DriverPoolSettings,
};
use oxyde_query::{build_sql, Dialect};
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyList, PyString, PyTuple};
use sea_query::Value as QueryValue;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// ============================================================================
// Mutation Results
// ============================================================================

/// Result of an INSERT operation (msgpack serializable)
#[derive(Serialize, Deserialize)]
struct InsertResult {
    affected: usize,
    inserted_ids: Vec<JsonValue>,
}

/// Result of an UPDATE or DELETE operation (msgpack serializable)
#[derive(Serialize, Deserialize)]
struct MutationResult {
    affected: u64,
}

/// Result of an UPDATE or DELETE operation with RETURNING clause (msgpack serializable)
/// Uses columnar format: (columns, rows) for memory efficiency
#[derive(Serialize, Deserialize)]
struct MutationWithReturningResult {
    affected: usize,
    columns: Vec<String>,
    rows: Vec<Vec<JsonValue>>,
}

/// ABI version for compatibility checking
const ABI_VERSION: u32 = 1;

#[pyfunction]
fn init_pool<'py>(
    py: Python<'py>,
    name: String,
    url: String,
    settings: Option<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let pool_settings = extract_pool_settings(py, settings)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_init_pool(&name, &url, pool_settings)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn init_pool_overwrite<'py>(
    py: Python<'py>,
    name: String,
    url: String,
    settings: Option<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let pool_settings = extract_pool_settings(py, settings)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_init_pool_overwrite(&name, &url, pool_settings)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn close_pool(py: Python<'_>, name: String) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_close_pool(&name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn close_all_pools(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_close_all_pools()
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn begin_transaction(py: Python<'_>, pool_name: String) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let id = driver_begin_transaction(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(id)
    })
}

#[pyfunction]
fn commit_transaction(py: Python<'_>, tx_id: u64) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_commit_transaction(tx_id)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn rollback_transaction(py: Python<'_>, tx_id: u64) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_rollback_transaction(tx_id)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn create_savepoint(
    py: Python<'_>,
    tx_id: u64,
    savepoint_name: String,
) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_create_savepoint(tx_id, &savepoint_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn rollback_to_savepoint(
    py: Python<'_>,
    tx_id: u64,
    savepoint_name: String,
) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_rollback_to_savepoint(tx_id, &savepoint_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn release_savepoint(
    py: Python<'_>,
    tx_id: u64,
    savepoint_name: String,
) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_release_savepoint(tx_id, &savepoint_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

/// Check if profiling is enabled via OXYDE_PROFILE env var
fn is_profiling_enabled() -> bool {
    std::env::var("OXYDE_PROFILE")
        .map(|v| v == "1")
        .unwrap_or(false)
}

#[pyfunction]
fn execute<'py>(
    py: Python<'py>,
    pool_name: String,
    ir_bytes: &Bound<'py, PyBytes>,
) -> PyResult<Bound<'py, PyAny>> {
    let ir_data = ir_bytes.as_bytes().to_vec();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let profile = is_profiling_enabled();
        let total_start = Instant::now();

        // Stage 1: Deserialize IR from msgpack
        let ir = QueryIR::from_msgpack(&ir_data)
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        // Stage 2: Validate IR
        ir.validate()
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        // Stage 3: Get backend/dialect
        let backend = driver_pool_backend(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        let dialect = backend_to_dialect(backend);

        // Stage 4: Build SQL
        let (sql, params) =
            build_sql(&ir, dialect).map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        let results = match ir.op {
            oxyde_codec::Operation::Select | oxyde_codec::Operation::Raw => {
                // Always use columnar format for memory efficiency
                let exec_start = Instant::now();
                let (columns, rows) =
                    execute_query_columnar(&pool_name, &sql, &params, ir.col_types.as_ref())
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                let exec_us = exec_start.elapsed().as_micros();
                let num_rows = rows.len();

                let serialize_start = Instant::now();
                let result = oxyde_codec::serialize_columnar_results((columns, rows))
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                let serialize_us = serialize_start.elapsed().as_micros();

                if profile {
                    let total_us = total_start.elapsed().as_micros();
                    eprintln!(
                        "[OXYDE_PROFILE] SELECT columnar ({} rows): exec={} µs, serialize={} µs, total={} µs, bytes={}",
                        num_rows, exec_us, serialize_us, total_us, result.len()
                    );
                }
                result
            }
            oxyde_codec::Operation::Insert => {
                // Single insert with RETURNING * (ir.returning=true) returns full rows
                // Bulk insert returns only PKs for efficiency
                if ir.returning.unwrap_or(false) {
                    // Single insert: use RETURNING * and return full row data
                    let (columns, rows) = execute_query_columnar(&pool_name, &sql, &params, None)
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    rmp_serde::to_vec_named(&MutationWithReturningResult {
                        affected: rows.len(),
                        columns,
                        rows,
                    })
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                } else {
                    // Bulk insert: return only PKs
                    let pk_column = ir.pk_column.as_deref();
                    let ids = execute_insert_returning(&pool_name, &sql, &params, pk_column)
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    rmp_serde::to_vec_named(&InsertResult {
                        affected: ids.len(),
                        inserted_ids: ids,
                    })
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                }
            }
            oxyde_codec::Operation::Update | oxyde_codec::Operation::Delete => {
                let op_name = if matches!(ir.op, oxyde_codec::Operation::Delete) {
                    "DELETE"
                } else {
                    "UPDATE"
                };

                // If RETURNING clause is requested, use columnar format
                if ir.returning.unwrap_or(false) {
                    let exec_start = Instant::now();
                    let (columns, rows) = execute_query_columnar(&pool_name, &sql, &params, None)
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                    let exec_us = exec_start.elapsed().as_micros();

                    let serialize_start = Instant::now();
                    let result = rmp_serde::to_vec_named(&MutationWithReturningResult {
                        affected: rows.len(),
                        columns,
                        rows,
                    })
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                    let serialize_us = serialize_start.elapsed().as_micros();

                    if profile {
                        let total_us = total_start.elapsed().as_micros();
                        eprintln!(
                            "[OXYDE_PROFILE] {} RETURNING: exec={} µs, serialize={} µs, total={} µs",
                            op_name, exec_us, serialize_us, total_us
                        );
                    }
                    result
                } else {
                    let exec_start = Instant::now();
                    let affected = execute_statement(&pool_name, &sql, &params)
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                    let exec_us = exec_start.elapsed().as_micros();

                    let serialize_start = Instant::now();
                    let result = rmp_serde::to_vec_named(&MutationResult { affected })
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                    let serialize_us = serialize_start.elapsed().as_micros();

                    if profile {
                        let total_us = total_start.elapsed().as_micros();
                        eprintln!(
                            "[OXYDE_PROFILE] {} (affected={}): exec={} µs, serialize={} µs, total={} µs, sql={}",
                            op_name, affected, exec_us, serialize_us, total_us, sql
                        );
                    }
                    result
                }
            }
        };

        Ok(results)
    })
}

#[pyfunction]
fn execute_in_transaction<'py>(
    py: Python<'py>,
    pool_name: String,
    tx_id: u64,
    ir_bytes: &Bound<'py, PyBytes>,
) -> PyResult<Bound<'py, PyAny>> {
    let ir_data = ir_bytes.as_bytes().to_vec();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let ir = QueryIR::from_msgpack(&ir_data)
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        ir.validate()
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        let backend = driver_pool_backend(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        let dialect = backend_to_dialect(backend);

        let (sql, params) =
            build_sql(&ir, dialect).map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        let results = match ir.op {
            oxyde_codec::Operation::Select | oxyde_codec::Operation::Raw => {
                // Always use columnar format
                let (columns, rows) = execute_query_columnar_in_transaction(
                    tx_id,
                    &sql,
                    &params,
                    ir.col_types.as_ref(),
                )
                .await
                .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                oxyde_codec::serialize_columnar_results((columns, rows))
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
            }
            oxyde_codec::Operation::Insert => {
                // Single insert with RETURNING * returns full rows
                // Bulk insert returns only PKs for efficiency
                if ir.returning.unwrap_or(false) {
                    let (columns, rows) =
                        execute_query_columnar_in_transaction(tx_id, &sql, &params, None)
                            .await
                            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    rmp_serde::to_vec_named(&MutationWithReturningResult {
                        affected: rows.len(),
                        columns,
                        rows,
                    })
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                } else {
                    let pk_column = ir.pk_column.as_deref();
                    let ids =
                        execute_insert_returning_in_transaction(tx_id, &sql, &params, pk_column)
                            .await
                            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    rmp_serde::to_vec_named(&InsertResult {
                        affected: ids.len(),
                        inserted_ids: ids,
                    })
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                }
            }
            oxyde_codec::Operation::Update | oxyde_codec::Operation::Delete => {
                // If RETURNING clause is requested, use columnar format
                if ir.returning.unwrap_or(false) {
                    let (columns, rows) =
                        execute_query_columnar_in_transaction(tx_id, &sql, &params, None)
                            .await
                            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    rmp_serde::to_vec_named(&MutationWithReturningResult {
                        affected: rows.len(),
                        columns,
                        rows,
                    })
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                } else {
                    let affected = execute_statement_in_transaction(tx_id, &sql, &params)
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    rmp_serde::to_vec_named(&MutationResult { affected })
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                }
            }
        };

        Ok(results)
    })
}

#[pyfunction]
fn render_sql<'py>(
    py: Python<'py>,
    pool_name: String,
    ir_bytes: &Bound<'py, PyBytes>,
) -> PyResult<Bound<'py, PyAny>> {
    let ir_data = ir_bytes.as_bytes().to_vec();
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let ir = QueryIR::from_msgpack(&ir_data)
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;
        ir.validate()
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        let backend = driver_pool_backend(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        let dialect = backend_to_dialect(backend);

        let (sql, params) =
            build_sql(&ir, dialect).map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        Python::attach(|py| -> PyResult<(String, Vec<Py<PyAny>>)> {
            let params_vec: Vec<Py<PyAny>> = params.iter().map(|v| value_to_py(py, v)).collect();
            Ok((sql, params_vec))
        })
    })
}

#[pyfunction]
fn render_sql_debug<'py>(
    py: Python<'py>,
    ir_bytes: &Bound<'py, PyBytes>,
    dialect_name: Option<&str>,
) -> PyResult<Bound<'py, PyTuple>> {
    let ir_data = ir_bytes.as_bytes();

    let ir =
        QueryIR::from_msgpack(ir_data).map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

    ir.validate()
        .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

    // Parse dialect name (default to Postgres)
    let dialect = match dialect_name {
        Some("postgres") | Some("postgresql") => Dialect::Postgres,
        Some("sqlite") => Dialect::Sqlite,
        Some("mysql") => Dialect::Mysql,
        None => Dialect::Postgres, // Default
        Some(other) => {
            return Err(PyErr::new::<PyValueError, _>(format!(
                "Unknown dialect '{}'. Use 'postgres', 'sqlite', or 'mysql'",
                other
            )))
        }
    };

    let (sql, params) =
        build_sql(&ir, dialect).map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

    let sql_obj = PyString::new(py, &sql);
    let params_obj = values_to_py(py, &params)?;
    PyTuple::new(py, &[sql_obj.into_any(), params_obj])
}

#[pyfunction]
fn explain<'py>(
    py: Python<'py>,
    pool_name: String,
    ir_bytes: &Bound<'py, PyBytes>,
    analyze: Option<bool>,
    format: Option<String>,
) -> PyResult<Bound<'py, PyAny>> {
    let ir_data = ir_bytes.as_bytes().to_vec();
    let format_token = format.unwrap_or_else(|| "text".to_string());
    let explain_format = if format_token.eq_ignore_ascii_case("json") {
        ExplainFormat::Json
    } else {
        ExplainFormat::Text
    };
    let explain_options = ExplainOptions {
        analyze: analyze.unwrap_or(false),
        format: explain_format,
    };

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let ir = QueryIR::from_msgpack(&ir_data)
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;
        ir.validate()
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        let backend = driver_pool_backend(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        let dialect = backend_to_dialect(backend);

        let (sql, params) =
            build_sql(&ir, dialect).map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        let plan = explain_query(&pool_name, &sql, &params, explain_options)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        Python::attach(|py| json_to_py(py, &plan))
    })
}

fn backend_to_dialect(backend: DatabaseBackend) -> Dialect {
    match backend {
        DatabaseBackend::Postgres => Dialect::Postgres,
        DatabaseBackend::MySql => Dialect::Mysql,
        DatabaseBackend::Sqlite => Dialect::Sqlite,
    }
}

fn extract_pool_settings(
    _py: Python<'_>,
    settings: Option<Bound<'_, PyAny>>,
) -> PyResult<DriverPoolSettings> {
    let mut parsed = DriverPoolSettings::default();

    let Some(obj) = settings else {
        return Ok(parsed);
    };

    if obj.is_none() {
        return Ok(parsed);
    }

    if let Ok(dict) = obj.downcast::<PyDict>() {
        parse_pool_dict(dict, &mut parsed)?;
        return Ok(parsed);
    }

    if obj.hasattr("to_payload")? {
        let payload = obj.call_method0("to_payload")?;
        if payload.is_none() {
            return Ok(parsed);
        }
        let dict = payload.downcast::<PyDict>()?;
        parse_pool_dict(dict, &mut parsed)?;
        return Ok(parsed);
    }

    let type_name = obj.get_type().name()?.to_string();
    Err(PyErr::new::<PyTypeError, _>(format!(
        "Pool settings must be a dict or expose to_payload(), got {}",
        type_name
    )))
}

fn values_to_py<'py>(py: Python<'py>, values: &[QueryValue]) -> PyResult<Bound<'py, PyAny>> {
    let list = PyList::empty(py);
    for value in values {
        list.append(value_to_py(py, value))?;
    }
    Ok(list.into_any())
}

#[allow(unreachable_patterns)]
fn value_to_py(py: Python<'_>, value: &QueryValue) -> Py<PyAny> {
    match value {
        QueryValue::Bool(Some(v)) => PyBool::new(py, *v).to_owned().unbind().into_any(),
        QueryValue::Bool(None) => py.None(),
        QueryValue::TinyInt(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::TinyInt(None) => py.None(),
        QueryValue::SmallInt(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::SmallInt(None) => py.None(),
        QueryValue::Int(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::Int(None) => py.None(),
        QueryValue::BigInt(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::BigInt(None) => py.None(),
        QueryValue::TinyUnsigned(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::TinyUnsigned(None) => py.None(),
        QueryValue::SmallUnsigned(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::SmallUnsigned(None) => py.None(),
        QueryValue::Unsigned(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::Unsigned(None) => py.None(),
        QueryValue::BigUnsigned(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::BigUnsigned(None) => py.None(),
        QueryValue::Float(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::Float(None) => py.None(),
        QueryValue::Double(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::Double(None) => py.None(),
        QueryValue::String(Some(s)) => PyString::new(py, s.as_str()).unbind().into_any(),
        QueryValue::String(None) => py.None(),
        QueryValue::Char(Some(c)) => {
            let text = c.to_string();
            PyString::new(py, &text).unbind().into_any()
        }
        QueryValue::Char(None) => py.None(),
        QueryValue::Bytes(Some(bytes)) => PyBytes::new(py, bytes.as_slice()).unbind().into_any(),
        QueryValue::Bytes(None) => py.None(),
        _ => PyString::new(py, &format!("{:?}", value))
            .unbind()
            .into_any(),
    }
}

fn json_to_py(py: Python<'_>, value: &JsonValue) -> PyResult<Py<PyAny>> {
    Ok(match value {
        JsonValue::Null => py.None(),
        JsonValue::Bool(v) => PyBool::new(py, *v).to_owned().unbind().into_any(),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_pyobject(py).unwrap().unbind().into_any()
            } else if let Some(u) = n.as_u64() {
                u.into_pyobject(py).unwrap().unbind().into_any()
            } else if let Some(f) = n.as_f64() {
                f.into_pyobject(py).unwrap().unbind().into_any()
            } else {
                py.None()
            }
        }
        JsonValue::String(s) => PyString::new(py, s).unbind().into_any(),
        JsonValue::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(json_to_py(py, item)?)?;
            }
            list.unbind().into_any()
        }
        JsonValue::Object(map) => {
            let dict = PyDict::new(py);
            for (key, val) in map {
                dict.set_item(key, json_to_py(py, val)?)?;
            }
            dict.unbind().into_any()
        }
    })
}

fn parse_pool_dict(dict: &Bound<'_, PyDict>, parsed: &mut DriverPoolSettings) -> PyResult<()> {
    if let Some(value) = dict.get_item("max_connections")? {
        parsed.max_connections = extract_optional_u32(&value)?;
    }
    if let Some(value) = dict.get_item("min_connections")? {
        parsed.min_connections = extract_optional_u32(&value)?;
    }
    if let Some(value) = dict.get_item("connect_timeout")? {
        parsed.acquire_timeout = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("idle_timeout")? {
        parsed.idle_timeout = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("acquire_timeout")? {
        parsed.acquire_timeout = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("max_lifetime")? {
        parsed.max_lifetime = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("test_before_acquire")? {
        parsed.test_before_acquire = extract_optional_bool(&value)?;
    }
    // Extract transaction cleanup settings
    if let Some(value) = dict.get_item("transaction_timeout")? {
        parsed.transaction_timeout = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("transaction_cleanup_interval")? {
        parsed.transaction_cleanup_interval = extract_optional_duration(&value)?;
    }

    // Extract SQLite PRAGMA settings
    if let Some(value) = dict.get_item("sqlite_journal_mode")? {
        parsed.sqlite_journal_mode = extract_optional_string(&value)?;
    }
    if let Some(value) = dict.get_item("sqlite_synchronous")? {
        parsed.sqlite_synchronous = extract_optional_string(&value)?;
    }
    if let Some(value) = dict.get_item("sqlite_cache_size")? {
        parsed.sqlite_cache_size = extract_optional_i32(&value)?;
    }
    if let Some(value) = dict.get_item("sqlite_busy_timeout")? {
        parsed.sqlite_busy_timeout = extract_optional_i32(&value)?;
    }

    Ok(())
}

fn extract_optional_u32(value: &Bound<'_, PyAny>) -> PyResult<Option<u32>> {
    if value.is_none() {
        Ok(None)
    } else {
        value
            .extract::<u32>()
            .map(Some)
            .map_err(|e| PyErr::new::<PyTypeError, _>(e.to_string()))
    }
}

fn extract_optional_i32(value: &Bound<'_, PyAny>) -> PyResult<Option<i32>> {
    if value.is_none() {
        Ok(None)
    } else {
        value
            .extract::<i32>()
            .map(Some)
            .map_err(|e| PyErr::new::<PyTypeError, _>(e.to_string()))
    }
}

fn extract_optional_string(value: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
    if value.is_none() {
        Ok(None)
    } else {
        value
            .extract::<String>()
            .map(Some)
            .map_err(|e| PyErr::new::<PyTypeError, _>(e.to_string()))
    }
}

fn extract_optional_bool(value: &Bound<'_, PyAny>) -> PyResult<Option<bool>> {
    if value.is_none() {
        Ok(None)
    } else {
        value
            .extract::<bool>()
            .map(Some)
            .map_err(|e| PyErr::new::<PyTypeError, _>(e.to_string()))
    }
}

fn extract_optional_duration(value: &Bound<'_, PyAny>) -> PyResult<Option<Duration>> {
    if value.is_none() {
        return Ok(None);
    }

    if let Ok(seconds) = value.extract::<f64>() {
        return Ok(Some(Duration::from_secs_f64(seconds)));
    }

    if let Ok(seconds) = value.extract::<u64>() {
        return Ok(Some(Duration::from_secs(seconds)));
    }

    if value.hasattr("total_seconds")? {
        let seconds_obj = value.call_method0("total_seconds")?;
        let seconds = seconds_obj.extract::<f64>()?;
        return Ok(Some(Duration::from_secs_f64(seconds)));
    }

    Err(PyErr::new::<PyTypeError, _>(
        "Duration must be provided as seconds (float/int) or datetime.timedelta".to_string(),
    ))
}

// ============================================================================
// Direct PyList conversion (no msgpack)
// ============================================================================

/// Convert columnar result (columns, rows) to PyList[PyDict]
fn columnar_to_pylist(
    py: Python<'_>,
    columns: Vec<String>,
    rows: Vec<Vec<JsonValue>>,
) -> PyResult<Py<PyList>> {
    let list = PyList::empty(py);
    for row in rows {
        let dict = PyDict::new(py);
        for (col, val) in columns.iter().zip(row.iter()) {
            dict.set_item(col, json_to_py(py, val)?)?;
        }
        list.append(dict)?;
    }
    Ok(list.unbind())
}

/// Execute SELECT query and return PyList[PyDict] directly (no msgpack)
/// NOTE: This version still uses Vec<Vec<JsonValue>> internally.
/// For memory-efficient version, use execute_select_direct.
#[pyfunction]
fn execute_to_pylist<'py>(
    py: Python<'py>,
    pool_name: String,
    ir_bytes: &Bound<'py, PyBytes>,
) -> PyResult<Bound<'py, PyAny>> {
    let ir_data = ir_bytes.as_bytes().to_vec();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        // Deserialize and validate IR
        let ir = QueryIR::from_msgpack(&ir_data)
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;
        ir.validate()
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        // Only support SELECT for this path
        if !matches!(
            ir.op,
            oxyde_codec::Operation::Select | oxyde_codec::Operation::Raw
        ) {
            return Err(PyErr::new::<PyValueError, _>(
                "execute_to_pylist only supports SELECT queries",
            ));
        }

        // Get dialect and build SQL
        let backend = driver_pool_backend(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        let dialect = backend_to_dialect(backend);
        let (sql, params) =
            build_sql(&ir, dialect).map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        // Execute query - returns columnar (columns, rows)
        let (columns, rows) =
            execute_query_columnar(&pool_name, &sql, &params, ir.col_types.as_ref())
                .await
                .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        // Convert to PyList[PyDict] - requires GIL
        Python::attach(|py| {
            let result = columnar_to_pylist(py, columns, rows)?;
            Ok(result)
        })
    })
}

/// Execute SELECT query with DIRECT Row -> PyDict conversion.
/// This skips the intermediate Vec<Vec<JsonValue>> step for lower memory usage.
#[pyfunction]
fn execute_select_direct<'py>(
    py: Python<'py>,
    pool_name: String,
    ir_bytes: &Bound<'py, PyBytes>,
) -> PyResult<Bound<'py, PyAny>> {
    use oxyde_driver::{
        get_pool, mysql_rows_to_pylist, pg_rows_to_pylist, sqlite_rows_to_pylist, DbPool,
    };

    let ir_data = ir_bytes.as_bytes().to_vec();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        // Deserialize and validate IR
        let ir = QueryIR::from_msgpack(&ir_data)
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;
        ir.validate()
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        // Only support SELECT for this path
        if !matches!(
            ir.op,
            oxyde_codec::Operation::Select | oxyde_codec::Operation::Raw
        ) {
            return Err(PyErr::new::<PyValueError, _>(
                "execute_select_direct only supports SELECT queries",
            ));
        }

        // Get pool and dialect
        let backend = driver_pool_backend(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        let dialect = backend_to_dialect(backend);
        let (sql, params) =
            build_sql(&ir, dialect).map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        // Get pool handle
        let pool = get_pool(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        // Execute and convert directly based on backend
        let col_types = ir.col_types.as_ref();

        match pool {
            DbPool::Sqlite(pool) => {
                use oxyde_driver::bind_sqlite;
                let query = bind_sqlite(sqlx::query(&sql), &params)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                let rows = query
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Query failed: {}", e)))?;

                // Direct conversion: SqliteRow -> PyDict (no JsonValue intermediate!)
                Python::attach(|py| {
                    sqlite_rows_to_pylist(py, rows, col_types).map(|list| list.unbind().into_any())
                })
            }
            DbPool::Postgres(pool) => {
                use oxyde_driver::bind_postgres;
                let query = bind_postgres(sqlx::query(&sql), &params)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                let rows = query
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Query failed: {}", e)))?;

                // Direct conversion: PgRow -> PyDict
                Python::attach(|py| {
                    pg_rows_to_pylist(py, rows, col_types).map(|list| list.unbind().into_any())
                })
            }
            DbPool::MySql(pool) => {
                use oxyde_driver::bind_mysql;
                let query = bind_mysql(sqlx::query(&sql), &params)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                let rows = query
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Query failed: {}", e)))?;

                // Direct conversion: MySqlRow -> PyDict
                Python::attach(|py| {
                    mysql_rows_to_pylist(py, rows, col_types).map(|list| list.unbind().into_any())
                })
            }
        }
    })
}

/// Execute SELECT query with batched PyDict conversion for lower peak memory usage.
/// Uses fetch_all() to release connection immediately, then converts to PyDict in batches.
#[pyfunction]
fn execute_select_batched<'py>(
    py: Python<'py>,
    pool_name: String,
    ir_bytes: &Bound<'py, PyBytes>,
    batch_size: Option<usize>,
) -> PyResult<Bound<'py, PyAny>> {
    use oxyde_driver::{
        bind_mysql, bind_postgres, bind_sqlite, decode_mysql_cell_to_py, decode_pg_cell_to_py,
        decode_sqlite_cell_to_py, extract_mysql_columns, extract_pg_columns,
        extract_sqlite_columns, get_pool, DbPool,
    };

    const DEFAULT_BATCH_SIZE: usize = 1000;
    let batch_size = batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
    let ir_data = ir_bytes.as_bytes().to_vec();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let ir = QueryIR::from_msgpack(&ir_data)
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;
        ir.validate()
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        if !matches!(
            ir.op,
            oxyde_codec::Operation::Select | oxyde_codec::Operation::Raw
        ) {
            return Err(PyErr::new::<PyValueError, _>(
                "execute_select_batched only supports SELECT queries",
            ));
        }

        let backend = driver_pool_backend(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        let dialect = backend_to_dialect(backend);
        let (sql, params) =
            build_sql(&ir, dialect).map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        let pool = get_pool(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        let col_types = ir.col_types.clone();

        match pool {
            DbPool::Sqlite(pool) => {
                let query = bind_sqlite(sqlx::query(&sql), &params)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                let rows = query
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Fetch failed: {}", e)))?;

                if rows.is_empty() {
                    return Python::attach(|py| Ok(PyList::empty(py).unbind().into_any()));
                }

                let columns = extract_sqlite_columns(&rows[0], col_types.as_ref());
                let result: Py<PyList> =
                    Python::attach(|py| Ok::<_, PyErr>(PyList::empty(py).unbind()))?;

                for chunk in rows.chunks(batch_size) {
                    Python::attach(|py| {
                        let result_list = result.bind(py);
                        for row in chunk {
                            let dict = PyDict::new(py);
                            for (i, col) in columns.iter().enumerate() {
                                dict.set_item(
                                    &col.name,
                                    decode_sqlite_cell_to_py(py, row, i, col),
                                )?;
                            }
                            result_list.append(dict)?;
                        }
                        Ok::<_, PyErr>(())
                    })?;
                }

                Ok(result.into_any())
            }
            DbPool::Postgres(pool) => {
                let query = bind_postgres(sqlx::query(&sql), &params)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                let rows = query
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Fetch failed: {}", e)))?;

                if rows.is_empty() {
                    return Python::attach(|py| Ok(PyList::empty(py).unbind().into_any()));
                }

                let columns = extract_pg_columns(&rows[0], col_types.as_ref());
                let result: Py<PyList> =
                    Python::attach(|py| Ok::<_, PyErr>(PyList::empty(py).unbind()))?;

                for chunk in rows.chunks(batch_size) {
                    Python::attach(|py| {
                        let result_list = result.bind(py);
                        for row in chunk {
                            let dict = PyDict::new(py);
                            for (i, col) in columns.iter().enumerate() {
                                dict.set_item(&col.name, decode_pg_cell_to_py(py, row, i, col))?;
                            }
                            result_list.append(dict)?;
                        }
                        Ok::<_, PyErr>(())
                    })?;
                }

                Ok(result.into_any())
            }
            DbPool::MySql(pool) => {
                let query = bind_mysql(sqlx::query(&sql), &params)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                let rows = query
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Fetch failed: {}", e)))?;

                if rows.is_empty() {
                    return Python::attach(|py| Ok(PyList::empty(py).unbind().into_any()));
                }

                let columns = extract_mysql_columns(&rows[0], col_types.as_ref());
                let result: Py<PyList> =
                    Python::attach(|py| Ok::<_, PyErr>(PyList::empty(py).unbind()))?;

                for chunk in rows.chunks(batch_size) {
                    Python::attach(|py| {
                        let result_list = result.bind(py);
                        for row in chunk {
                            let dict = PyDict::new(py);
                            for (i, col) in columns.iter().enumerate() {
                                dict.set_item(&col.name, decode_mysql_cell_to_py(py, row, i, col))?;
                            }
                            result_list.append(dict)?;
                        }
                        Ok::<_, PyErr>(())
                    })?;
                }

                Ok(result.into_any())
            }
        }
    })
}

/// Execute SELECT with JOIN and return deduplicated structure:
/// {"main": [...], "relations": {"relation_name": {pk: {...}, ...}, ...}}
///
/// Uses fetch_all() + batched conversion for all databases.
#[pyfunction]
fn execute_select_batched_dedup<'py>(
    py: Python<'py>,
    pool_name: String,
    ir_bytes: &Bound<'py, PyBytes>,
    batch_size: Option<usize>,
) -> PyResult<Bound<'py, PyAny>> {
    use oxyde_driver::{
        bind_mysql, bind_postgres, bind_sqlite, decode_mysql_cell_to_py, decode_pg_cell_to_py,
        decode_sqlite_cell_to_py, extract_mysql_columns, extract_pg_columns,
        extract_sqlite_columns, get_pool, DbPool,
    };

    const DEFAULT_BATCH_SIZE: usize = 1000;
    let batch_size = batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
    let ir_data = ir_bytes.as_bytes().to_vec();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let ir = QueryIR::from_msgpack(&ir_data)
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;
        ir.validate()
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        if !matches!(ir.op, oxyde_codec::Operation::Select) {
            return Err(PyErr::new::<PyValueError, _>(
                "execute_select_batched_dedup only supports SELECT queries with joins",
            ));
        }

        let joins = ir.joins.as_ref().ok_or_else(|| {
            PyErr::new::<PyValueError, _>("execute_select_batched_dedup requires joins in IR")
        })?;

        let join_prefixes: Vec<(String, String, String)> = joins
            .iter()
            .map(|j| {
                (
                    format!("{}__", j.result_prefix),
                    j.path.clone(),
                    j.target_column.clone(),
                )
            })
            .collect();

        let backend = driver_pool_backend(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        let dialect = backend_to_dialect(backend);
        let (sql, params) =
            build_sql(&ir, dialect).map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        let pool = get_pool(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

        let col_types = ir.col_types.clone();

        // Helper to create empty result
        fn empty_dedup_result(py: Python<'_>) -> PyResult<Py<PyAny>> {
            let result = PyDict::new(py);
            result.set_item("main", PyList::empty(py))?;
            result.set_item("relations", PyDict::new(py))?;
            Ok(result.unbind().into_any())
        }

        // Helper to init result structure
        fn init_dedup_result(
            py: Python<'_>,
            join_prefixes: &[(String, String, String)],
        ) -> PyResult<(Py<PyList>, Py<PyDict>, Py<PyDict>)> {
            let relations = PyDict::new(py);
            for (_, path, _) in join_prefixes {
                relations.set_item(path, PyDict::new(py))?;
            }
            let seen_main_pks = PyDict::new(py);
            Ok((
                PyList::empty(py).unbind(),
                relations.unbind(),
                seen_main_pks.unbind(),
            ))
        }

        // Helper to finalize result
        fn finalize_dedup_result(
            py: Python<'_>,
            main_list: &Py<PyList>,
            relations_dict: &Py<PyDict>,
        ) -> PyResult<Py<PyDict>> {
            let result = PyDict::new(py);
            result.set_item("main", main_list.bind(py))?;
            result.set_item("relations", relations_dict.bind(py))?;
            Ok(result.unbind())
        }

        // Get main table PK column for deduplication
        let main_pk_column = ir.pk_column.clone();

        match pool {
            DbPool::Sqlite(pool) => {
                let query = bind_sqlite(sqlx::query(&sql), &params)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                let rows = query
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Fetch failed: {}", e)))?;

                if rows.is_empty() {
                    return Python::attach(empty_dedup_result);
                }

                let columns = extract_sqlite_columns(&rows[0], col_types.as_ref());
                let (main_list, relations_dict, seen_main_pks) =
                    Python::attach(|py| init_dedup_result(py, &join_prefixes))?;

                for chunk in rows.chunks(batch_size) {
                    Python::attach(|py| {
                        let main = main_list.bind(py);
                        let relations = relations_dict.bind(py);
                        let seen_pks = seen_main_pks.bind(py);
                        for row in chunk {
                            process_row_dedup_generic(
                                py,
                                &columns,
                                &join_prefixes,
                                main_pk_column.as_deref(),
                                main,
                                relations,
                                seen_pks,
                                |idx, col| decode_sqlite_cell_to_py(py, row, idx, col),
                            )?;
                        }
                        Ok::<_, PyErr>(())
                    })?;
                }

                Python::attach(|py| {
                    finalize_dedup_result(py, &main_list, &relations_dict).map(|r| r.into_any())
                })
            }
            DbPool::Postgres(pool) => {
                let query = bind_postgres(sqlx::query(&sql), &params)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                let rows = query
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Fetch failed: {}", e)))?;

                if rows.is_empty() {
                    return Python::attach(empty_dedup_result);
                }

                let columns = extract_pg_columns(&rows[0], col_types.as_ref());
                let (main_list, relations_dict, seen_main_pks) =
                    Python::attach(|py| init_dedup_result(py, &join_prefixes))?;

                for chunk in rows.chunks(batch_size) {
                    Python::attach(|py| {
                        let main = main_list.bind(py);
                        let relations = relations_dict.bind(py);
                        let seen_pks = seen_main_pks.bind(py);
                        for row in chunk {
                            process_row_dedup_generic(
                                py,
                                &columns,
                                &join_prefixes,
                                main_pk_column.as_deref(),
                                main,
                                relations,
                                seen_pks,
                                |idx, col| decode_pg_cell_to_py(py, row, idx, col),
                            )?;
                        }
                        Ok::<_, PyErr>(())
                    })?;
                }

                Python::attach(|py| {
                    finalize_dedup_result(py, &main_list, &relations_dict).map(|r| r.into_any())
                })
            }
            DbPool::MySql(pool) => {
                let query = bind_mysql(sqlx::query(&sql), &params)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                let rows = query
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Fetch failed: {}", e)))?;

                if rows.is_empty() {
                    return Python::attach(empty_dedup_result);
                }

                let columns = extract_mysql_columns(&rows[0], col_types.as_ref());
                let (main_list, relations_dict, seen_main_pks) =
                    Python::attach(|py| init_dedup_result(py, &join_prefixes))?;

                for chunk in rows.chunks(batch_size) {
                    Python::attach(|py| {
                        let main = main_list.bind(py);
                        let relations = relations_dict.bind(py);
                        let seen_pks = seen_main_pks.bind(py);
                        for row in chunk {
                            process_row_dedup_generic(
                                py,
                                &columns,
                                &join_prefixes,
                                main_pk_column.as_deref(),
                                main,
                                relations,
                                seen_pks,
                                |idx, col| decode_mysql_cell_to_py(py, row, idx, col),
                            )?;
                        }
                        Ok::<_, PyErr>(())
                    })?;
                }

                Python::attach(|py| {
                    finalize_dedup_result(py, &main_list, &relations_dict).map(|r| r.into_any())
                })
            }
        }
    })
}

/// Generic row processing for dedup - works with any database via closure
#[allow(clippy::too_many_arguments)]
fn process_row_dedup_generic<F>(
    py: Python<'_>,
    columns: &[oxyde_driver::StreamingColumnMeta],
    join_prefixes: &[(String, String, String)],
    main_pk_column: Option<&str>,
    main_list: &Bound<'_, PyList>,
    relations_dict: &Bound<'_, PyDict>,
    seen_main_pks: &Bound<'_, PyDict>,
    decode_cell: F,
) -> PyResult<()>
where
    F: Fn(usize, &oxyde_driver::StreamingColumnMeta) -> Py<PyAny>,
{
    let main_dict = PyDict::new(py);

    for (i, col) in columns.iter().enumerate() {
        let mut is_relation = false;

        for (prefix, path, pk_col) in join_prefixes {
            if col.name.starts_with(prefix) {
                if let Some(rel_dict) = relations_dict.get_item(path)? {
                    let rel_dict = rel_dict.downcast::<PyDict>()?;

                    let pk_col_full = format!("{}{}", prefix, pk_col);
                    if let Some(pk_idx) = columns.iter().position(|c| c.name == pk_col_full) {
                        let pk_value = decode_cell(pk_idx, &columns[pk_idx]);

                        // Skip NULL PKs (LEFT JOIN with no match)
                        if pk_value.bind(py).is_none() {
                            is_relation = true;
                            break;
                        }

                        if rel_dict.get_item(&pk_value)?.is_none() {
                            let entry = PyDict::new(py);
                            for (j, jcol) in columns.iter().enumerate() {
                                if jcol.name.starts_with(prefix) {
                                    let rel_name = &jcol.name[prefix.len()..];
                                    entry.set_item(rel_name, decode_cell(j, jcol))?;
                                }
                            }
                            rel_dict.set_item(&pk_value, entry)?;
                        }
                    }
                }
                is_relation = true;
                break;
            }
        }

        if !is_relation {
            main_dict.set_item(&col.name, decode_cell(i, col))?;
        }
    }

    // Deduplicate main objects by PK
    if let Some(pk_col) = main_pk_column {
        if let Some(pk_idx) = columns.iter().position(|c| c.name == pk_col) {
            let pk_value = decode_cell(pk_idx, &columns[pk_idx]);
            // Only append if we haven't seen this PK before
            if seen_main_pks.get_item(&pk_value)?.is_none() {
                seen_main_pks.set_item(&pk_value, true)?;
                main_list.append(main_dict)?;
            }
        } else {
            // PK column not found, append without dedup
            main_list.append(main_dict)?;
        }
    } else {
        // No PK specified, append without dedup
        main_list.append(main_dict)?;
    }

    Ok(())
}

// ============================================================================
// Migration functions
// ============================================================================

/// Compute diff between two schema snapshots (JSON)
///
/// Args:
///     old_json: Old schema snapshot as JSON string
///     new_json: New schema snapshot as JSON string
///
/// Returns:
///     JSON string with list of migration operations
#[pyfunction]
fn migration_compute_diff(old_json: &str, new_json: &str) -> PyResult<String> {
    use oxyde_migrate::{compute_diff, Snapshot};

    let old = Snapshot::from_json(old_json).map_err(|e| {
        PyErr::new::<PyValueError, _>(format!("Failed to parse old snapshot: {}", e))
    })?;

    let new = Snapshot::from_json(new_json).map_err(|e| {
        PyErr::new::<PyValueError, _>(format!("Failed to parse new snapshot: {}", e))
    })?;

    let ops = compute_diff(&old, &new);

    serde_json::to_string(&ops).map_err(|e| {
        PyErr::new::<PyValueError, _>(format!("Failed to serialize operations: {}", e))
    })
}

/// Convert migration operations to SQL statements
///
/// Args:
///     operations_json: JSON string with list of migration operations
///     dialect: Database dialect ("sqlite", "postgres", or "mysql")
///
/// Returns:
///     List of SQL statements
#[pyfunction]
fn migration_to_sql(operations_json: &str, dialect: &str) -> PyResult<Vec<String>> {
    use oxyde_migrate::{Dialect, MigrationOp};

    let ops: Vec<MigrationOp> = serde_json::from_str(operations_json)
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("Failed to parse operations: {}", e)))?;

    let dialect_enum = match dialect {
        "sqlite" => Dialect::Sqlite,
        "postgres" => Dialect::Postgres,
        "mysql" => Dialect::Mysql,
        _ => {
            return Err(PyErr::new::<PyValueError, _>(format!(
                "Invalid dialect: {}",
                dialect
            )))
        }
    };

    let mut all_sql = Vec::new();
    for op in &ops {
        let sqls = op
            .to_sql(dialect_enum)
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Migration error: {}", e)))?;
        all_sql.extend(sqls);
    }
    Ok(all_sql)
}

// ============================================================================
// Python module definition
// ============================================================================

/// Python module definition
#[pymodule]
fn _oxyde_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__abi_version__", ABI_VERSION)?;

    m.add_function(wrap_pyfunction!(init_pool, m)?)?;
    m.add_function(wrap_pyfunction!(init_pool_overwrite, m)?)?;
    m.add_function(wrap_pyfunction!(close_pool, m)?)?;
    m.add_function(wrap_pyfunction!(close_all_pools, m)?)?;
    m.add_function(wrap_pyfunction!(execute, m)?)?;
    m.add_function(wrap_pyfunction!(execute_to_pylist, m)?)?;
    m.add_function(wrap_pyfunction!(execute_select_direct, m)?)?;
    m.add_function(wrap_pyfunction!(execute_select_batched, m)?)?;
    m.add_function(wrap_pyfunction!(execute_select_batched_dedup, m)?)?;
    m.add_function(wrap_pyfunction!(begin_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(commit_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(rollback_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(create_savepoint, m)?)?;
    m.add_function(wrap_pyfunction!(rollback_to_savepoint, m)?)?;
    m.add_function(wrap_pyfunction!(release_savepoint, m)?)?;
    m.add_function(wrap_pyfunction!(execute_in_transaction, m)?)?;
    m.add_function(wrap_pyfunction!(render_sql, m)?)?;
    m.add_function(wrap_pyfunction!(render_sql_debug, m)?)?;
    m.add_function(wrap_pyfunction!(explain, m)?)?;

    // Migration functions
    m.add_function(wrap_pyfunction!(migration_compute_diff, m)?)?;
    m.add_function(wrap_pyfunction!(migration_to_sql, m)?)?;

    Ok(())
}

#[cfg(all(test, not(feature = "extension-module")))]
mod tests {
    use super::*;
    use pyo3::types::PyDict;

    #[test]
    fn test_extract_pool_settings_from_dict() {
        pyo3::prepare_freethreaded_python();
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("max_connections", 20u32).unwrap();
            dict.set_item("min_connections", 5u32).unwrap();
            dict.set_item("connect_timeout", 1.5f64).unwrap();
            dict.set_item("idle_timeout", 2.0f64).unwrap();
            dict.set_item("acquire_timeout", 3.0f64).unwrap();
            dict.set_item("max_lifetime", 4.0f64).unwrap();
            dict.set_item("test_before_acquire", true).unwrap();

            let settings = extract_pool_settings(py, Some(dict.into_any())).unwrap();
            assert_eq!(settings.max_connections, Some(20));
            assert_eq!(settings.min_connections, Some(5));
            assert!((settings.acquire_timeout.unwrap().as_secs_f64() - 3.0).abs() < f64::EPSILON);
            assert!((settings.idle_timeout.unwrap().as_secs_f64() - 2.0).abs() < f64::EPSILON);
            assert!((settings.max_lifetime.unwrap().as_secs_f64() - 4.0).abs() < f64::EPSILON);
            assert_eq!(settings.test_before_acquire, Some(true));
        });
    }

    #[test]
    fn test_extract_pool_settings_rejects_invalid_type() {
        pyo3::prepare_freethreaded_python();
        Python::attach(|py| {
            let value = "invalid".into_pyobject(py).unwrap().into_any();
            let err = extract_pool_settings(py, Some(value)).unwrap_err();
            assert!(err.to_string().contains("Pool settings must be a dict"));
        });
    }
}

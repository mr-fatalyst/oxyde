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

use std::collections::HashMap;
use std::time::Duration;

use oxyde_codec::QueryIR;
use oxyde_driver::{
    begin_transaction as driver_begin_transaction, close_all_pools as driver_close_all_pools,
    close_pool as driver_close_pool, commit_transaction as driver_commit_transaction,
    create_savepoint as driver_create_savepoint, execute_insert_returning,
    execute_insert_returning_in_transaction, execute_query, execute_query_in_transaction,
    execute_statement, execute_statement_in_transaction, explain_query,
    init_pool as driver_init_pool, init_pool_overwrite as driver_init_pool_overwrite,
    pool_backend as driver_pool_backend, release_savepoint as driver_release_savepoint,
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
#[derive(Serialize, Deserialize)]
struct MutationWithReturningResult {
    affected: usize,
    rows: Vec<HashMap<String, JsonValue>>,
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

#[pyfunction]
fn execute<'py>(
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

        let results = match ir.op {
            oxyde_codec::Operation::Select | oxyde_codec::Operation::Raw => {
                // Raw SQL and SELECT both return rows
                let rows = execute_query(&pool_name, &sql, &params)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                oxyde_codec::serialize_results(rows)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
            }
            oxyde_codec::Operation::Insert => {
                // Get pk_column from IR (defaults to "id" in driver if None)
                let pk_column = ir.pk_column.as_deref();

                // Execute INSERT and return inserted IDs (works for both single and bulk)
                let ids = execute_insert_returning(&pool_name, &sql, &params, pk_column)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                rmp_serde::to_vec_named(&InsertResult {
                    affected: ids.len(),
                    inserted_ids: ids,
                })
                .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
            }
            oxyde_codec::Operation::Update | oxyde_codec::Operation::Delete => {
                // If RETURNING clause is requested, use execute_query to get rows back
                if ir.returning.unwrap_or(false) {
                    let rows = execute_query(&pool_name, &sql, &params)
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    rmp_serde::to_vec_named(&MutationWithReturningResult {
                        affected: rows.len(),
                        rows,
                    })
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                } else {
                    let affected = execute_statement(&pool_name, &sql, &params)
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
                // Raw SQL and SELECT both return rows
                let rows = execute_query_in_transaction(tx_id, &sql, &params)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                oxyde_codec::serialize_results(rows)
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
            }
            oxyde_codec::Operation::Insert => {
                // Get pk_column from IR (defaults to "id" in driver if None)
                let pk_column = ir.pk_column.as_deref();

                // INSERT - return inserted IDs
                let ids = execute_insert_returning_in_transaction(tx_id, &sql, &params, pk_column)
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                rmp_serde::to_vec_named(&InsertResult {
                    affected: ids.len(),
                    inserted_ids: ids,
                })
                .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
            }
            oxyde_codec::Operation::Update | oxyde_codec::Operation::Delete => {
                // If RETURNING clause is requested, use execute_query to get rows back
                if ir.returning.unwrap_or(false) {
                    let rows = execute_query_in_transaction(tx_id, &sql, &params)
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    rmp_serde::to_vec_named(&MutationWithReturningResult {
                        affected: rows.len(),
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
        Python::with_gil(|py| {
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
        Python::with_gil(|py| {
            let value = "invalid".into_pyobject(py).unwrap().into_any();
            let err = extract_pool_settings(py, Some(value)).unwrap_err();
            assert!(err.to_string().contains("Pool settings must be a dict"));
        });
    }
}

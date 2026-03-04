//! Query execution: execute, execute_in_transaction, render_sql, explain.

use std::time::Instant;

use crate::convert::{backend_to_dialect, json_to_py, value_to_py, values_to_py};
use crate::types::{InsertResult, MutationResult, MutationWithReturningResult};
use oxyde_codec::QueryIR;
use oxyde_driver::{
    execute_insert_returning, execute_insert_returning_in_transaction, execute_query_columnar,
    execute_query_columnar_in_transaction, execute_statement, execute_statement_in_transaction,
    explain_query, pool_backend as driver_pool_backend, ExplainFormat, ExplainOptions,
};
use oxyde_query::{build_sql, Dialect};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString, PyTuple};

/// Check if profiling is enabled via OXYDE_PROFILE env var
fn is_profiling_enabled() -> bool {
    std::env::var("OXYDE_PROFILE")
        .map(|v| v == "1")
        .unwrap_or(false)
}

#[pyfunction]
pub(crate) fn execute<'py>(
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
pub(crate) fn execute_in_transaction<'py>(
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
pub(crate) fn render_sql<'py>(
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
pub(crate) fn render_sql_debug<'py>(
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
pub(crate) fn explain<'py>(
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

//! Query execution: execute, execute_in_transaction, render_sql, explain.

use std::time::Instant;

use crate::convert::{
    backend_to_dialect, json_to_py, value_to_py, values_to_py, values_to_py_tagged,
};
use crate::types::{encode_insert_result, encode_mutation_result};
use oxyde_codec::QueryIR;
use oxyde_driver::{
    execute_insert_returning, execute_insert_returning_in_transaction, execute_mutation_returning,
    execute_mutation_returning_in_transaction, execute_query_columnar,
    execute_query_columnar_dedup, execute_query_columnar_dedup_in_transaction,
    execute_query_columnar_in_transaction, execute_statement, execute_statement_in_transaction,
    explain_query, pool_backend as driver_pool_backend, ExplainFormat, ExplainOptions,
    RelationInfo,
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

fn should_use_returning_path(returning: Option<bool>, sql: &str) -> bool {
    returning.unwrap_or(false) && sql.contains("RETURNING")
}

/// Execute a query: deserialize IR → validate → build SQL → execute → msgpack bytes → Coroutine[bytes].
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
                let exec_start = Instant::now();
                let is_scalar = ir.count.unwrap_or(false) || ir.exists.unwrap_or(false);
                let (result, num_rows) = if let (false, Some(joins)) = (is_scalar, &ir.joins) {
                    let relations: Vec<RelationInfo> = joins
                        .iter()
                        .map(|j| RelationInfo {
                            prefix: j.result_prefix.clone(),
                            pk_col: format!("{}__{}", j.result_prefix, j.target_column),
                        })
                        .collect();
                    execute_query_columnar_dedup(
                        &pool_name,
                        &sql,
                        &params,
                        ir.col_types.as_ref(),
                        &relations,
                    )
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                } else {
                    execute_query_columnar(&pool_name, &sql, &params, ir.col_types.as_ref())
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                };
                let exec_us = exec_start.elapsed().as_micros();

                if profile {
                    let total_us = total_start.elapsed().as_micros();
                    eprintln!(
                        "[OXYDE_PROFILE] SELECT columnar ({} rows): exec={} µs, total={} µs, bytes={}",
                        num_rows, exec_us, total_us, result.len()
                    );
                }
                result
            }
            oxyde_codec::Operation::Insert => {
                // Single insert with RETURNING * returns full rows (Postgres/SQLite).
                // MySQL doesn't support RETURNING the query builder omits it,
                // so we fall through to execute_insert_returning which uses last_insert_id().
                if should_use_returning_path(ir.returning, &sql) {
                    execute_mutation_returning(&pool_name, &sql, &params, ir.col_types.as_ref())
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                } else {
                    // Bulk insert: return only PKs
                    let pk_column = ir.pk_column.as_deref();
                    let ids = execute_insert_returning(&pool_name, &sql, &params, pk_column)
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    encode_insert_result(ids.len(), &ids)
                }
            }
            oxyde_codec::Operation::Update | oxyde_codec::Operation::Delete => {
                let op_name = if matches!(ir.op, oxyde_codec::Operation::Delete) {
                    "DELETE"
                } else {
                    "UPDATE"
                };

                // Use mutation returning only when SQL actually contains RETURNING
                // (Postgres/SQLite). MySQL never gets RETURNING in SQL, so fall
                // through to execute_statement to preserve affected-row count.
                if should_use_returning_path(ir.returning, &sql) {
                    let exec_start = Instant::now();
                    let result = execute_mutation_returning(
                        &pool_name,
                        &sql,
                        &params,
                        ir.col_types.as_ref(),
                    )
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
                    let exec_us = exec_start.elapsed().as_micros();

                    if profile {
                        let total_us = total_start.elapsed().as_micros();
                        eprintln!(
                            "[OXYDE_PROFILE] {} RETURNING: exec={} µs, total={} µs",
                            op_name, exec_us, total_us
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
                    let result = encode_mutation_result(affected);
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

/// Execute a query within a transaction → Coroutine[bytes].
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
                let is_scalar = ir.count.unwrap_or(false) || ir.exists.unwrap_or(false);
                let (result, _num_rows) = if let (false, Some(joins)) = (is_scalar, &ir.joins) {
                    let relations: Vec<RelationInfo> = joins
                        .iter()
                        .map(|j| RelationInfo {
                            prefix: j.result_prefix.clone(),
                            pk_col: format!("{}__{}", j.result_prefix, j.target_column),
                        })
                        .collect();
                    execute_query_columnar_dedup_in_transaction(
                        tx_id,
                        &sql,
                        &params,
                        ir.col_types.as_ref(),
                        &relations,
                    )
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                } else {
                    execute_query_columnar_in_transaction(
                        tx_id,
                        &sql,
                        &params,
                        ir.col_types.as_ref(),
                    )
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                };
                result
            }
            oxyde_codec::Operation::Insert => {
                if should_use_returning_path(ir.returning, &sql) {
                    execute_mutation_returning_in_transaction(
                        tx_id,
                        &sql,
                        &params,
                        ir.col_types.as_ref(),
                    )
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                } else {
                    let pk_column = ir.pk_column.as_deref();
                    let ids =
                        execute_insert_returning_in_transaction(tx_id, &sql, &params, pk_column)
                            .await
                            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    encode_insert_result(ids.len(), &ids)
                }
            }
            oxyde_codec::Operation::Update | oxyde_codec::Operation::Delete => {
                if should_use_returning_path(ir.returning, &sql) {
                    execute_mutation_returning_in_transaction(
                        tx_id,
                        &sql,
                        &params,
                        ir.col_types.as_ref(),
                    )
                    .await
                    .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?
                } else {
                    let affected = execute_statement_in_transaction(tx_id, &sql, &params)
                        .await
                        .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;

                    encode_mutation_result(affected)
                }
            }
        };

        Ok(results)
    })
}

#[cfg(test)]
mod tests {
    use super::should_use_returning_path;

    #[test]
    fn returning_path_requires_flag_and_sql_clause() {
        assert!(should_use_returning_path(
            Some(true),
            "INSERT ... RETURNING *"
        ));
        assert!(!should_use_returning_path(Some(true), "INSERT ..."));
        assert!(!should_use_returning_path(
            Some(false),
            "INSERT ... RETURNING *"
        ));
        assert!(!should_use_returning_path(None, "INSERT ... RETURNING *"));
    }
}

/// Render SQL from IR without executing (requires pool for dialect detection) → Coroutine[(str, list)].
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

/// Render SQL from IR without pool (sync, defaults to Postgres dialect) → (str, list).
///
/// When `with_types` is true, params are returned as list of (type_tag, value) tuples
/// instead of plain values. This allows tests to verify the exact sea_query Value
/// variant used for parameter binding.
#[pyfunction]
#[pyo3(signature = (ir_bytes, dialect_name=None, with_types=false))]
pub(crate) fn render_sql_debug<'py>(
    py: Python<'py>,
    ir_bytes: &Bound<'py, PyBytes>,
    dialect_name: Option<&str>,
    with_types: bool,
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
    let params_obj = if with_types {
        values_to_py_tagged(py, &params)?
    } else {
        values_to_py(py, &params)?
    };
    PyTuple::new(py, &[sql_obj.into_any(), params_obj])
}

/// Run EXPLAIN (ANALYZE) on a query, return plan as JSON or text → Coroutine.
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

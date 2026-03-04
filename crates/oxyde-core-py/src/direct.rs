//! Direct PyList conversion paths (no msgpack on output).
//!
//! Provides multiple execution strategies for SELECT queries:
//! - `execute_to_pylist`: columnar → PyList[PyDict] (via JsonValue intermediate)
//! - `execute_select_direct`: Row → PyDict directly (no JsonValue intermediate)
//! - `execute_select_batched`: Row → PyDict in batches for lower peak memory
//! - `execute_select_batched_dedup`: batched with JOIN deduplication

use crate::convert::{backend_to_dialect, json_to_py};
use oxyde_codec::QueryIR;
use oxyde_driver::{
    execute_query_columnar, pool_backend as driver_pool_backend, StreamingColumnMeta,
};
use oxyde_query::build_sql;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};
use serde_json::Value as JsonValue;

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
pub(crate) fn execute_to_pylist<'py>(
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
pub(crate) fn execute_select_direct<'py>(
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
pub(crate) fn execute_select_batched<'py>(
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
pub(crate) fn execute_select_batched_dedup<'py>(
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
    columns: &[StreamingColumnMeta],
    join_prefixes: &[(String, String, String)],
    main_pk_column: Option<&str>,
    main_list: &Bound<'_, PyList>,
    relations_dict: &Bound<'_, PyDict>,
    seen_main_pks: &Bound<'_, PyDict>,
    decode_cell: F,
) -> PyResult<()>
where
    F: Fn(usize, &StreamingColumnMeta) -> Py<PyAny>,
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

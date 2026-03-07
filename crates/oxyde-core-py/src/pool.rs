//! Pool management and transaction PyO3 wrappers.

use crate::convert::extract_pool_settings;
use oxyde_driver::{
    begin_transaction as driver_begin_transaction, close_all_pools as driver_close_all_pools,
    close_pool as driver_close_pool, commit_transaction as driver_commit_transaction,
    create_savepoint as driver_create_savepoint, init_pool as driver_init_pool,
    init_pool_overwrite as driver_init_pool_overwrite,
    release_savepoint as driver_release_savepoint,
    rollback_to_savepoint as driver_rollback_to_savepoint,
    rollback_transaction as driver_rollback_transaction,
};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

/// Initialize a named connection pool → Coroutine.
#[pyfunction]
pub(crate) fn init_pool<'py>(
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

/// Initialize a named connection pool, replacing existing if present → Coroutine.
#[pyfunction]
pub(crate) fn init_pool_overwrite<'py>(
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

/// Close and remove a named connection pool → Coroutine.
#[pyfunction]
pub(crate) fn close_pool(py: Python<'_>, name: String) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_close_pool(&name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

/// Close all connection pools, rolling back active transactions → Coroutine.
#[pyfunction]
pub(crate) fn close_all_pools(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_close_all_pools()
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

/// Begin a new transaction on the named pool → Coroutine[int] (transaction ID).
#[pyfunction]
pub(crate) fn begin_transaction(py: Python<'_>, pool_name: String) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let id = driver_begin_transaction(&pool_name)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(id)
    })
}

/// Commit a transaction → Coroutine.
#[pyfunction]
pub(crate) fn commit_transaction(py: Python<'_>, tx_id: u64) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_commit_transaction(tx_id)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

/// Rollback a transaction → Coroutine.
#[pyfunction]
pub(crate) fn rollback_transaction(py: Python<'_>, tx_id: u64) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        driver_rollback_transaction(tx_id)
            .await
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(e.to_string()))?;
        Ok(())
    })
}

/// Create a named savepoint within a transaction → Coroutine.
#[pyfunction]
pub(crate) fn create_savepoint(
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

/// Rollback to a named savepoint → Coroutine.
#[pyfunction]
pub(crate) fn rollback_to_savepoint(
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

/// Release a savepoint → Coroutine.
#[pyfunction]
pub(crate) fn release_savepoint(
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

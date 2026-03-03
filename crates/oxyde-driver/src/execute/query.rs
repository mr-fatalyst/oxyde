//! Query and statement execution (pool and transaction paths).

use sea_query::Value;
use std::collections::HashMap;
use std::time::Instant;
use tracing::debug;

use crate::convert::ColumnarResult;
use crate::error::{DriverError, Result};
use crate::execute::traits::{ConnExec, PoolExec};
use crate::{registry, transaction_registry};

/// Check if profiling is enabled via OXYDE_PROFILE env var
fn is_profiling_enabled() -> bool {
    std::env::var("OXYDE_PROFILE")
        .map(|v| v == "1")
        .unwrap_or(false)
}

pub async fn execute_query(
    pool_name: &str,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
) -> Result<Vec<HashMap<String, serde_json::Value>>> {
    debug!(
        "Executing query on '{}': {} ({} params, col_types: {})",
        pool_name,
        sql,
        params.len(),
        col_types.is_some()
    );

    let profile = is_profiling_enabled();
    let handle = registry().get(pool_name).await?;
    let pool = handle.clone_pool();

    let start = Instant::now();
    let results = pool.query(sql, params, col_types).await?;
    let elapsed_us = start.elapsed().as_micros();

    if profile {
        eprintln!(
            "[OXYDE_PROFILE] execute_query ({}, {} rows): total={} µs",
            pool.backend_name(),
            results.len(),
            elapsed_us
        );
    }
    debug!(
        "Query on '{}' ({}) returned {} rows",
        pool_name,
        pool.backend_name(),
        results.len()
    );
    Ok(results)
}

/// Execute a SELECT query and return results in columnar format.
/// This is more memory-efficient than execute_query for large result sets:
/// - Column names stored once instead of per-row
/// - No HashMap overhead per row
/// - Smaller msgpack serialization
pub async fn execute_query_columnar(
    pool_name: &str,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
) -> Result<ColumnarResult> {
    debug!(
        "Executing columnar query on '{}': {} ({} params)",
        pool_name,
        sql,
        params.len()
    );

    let profile = is_profiling_enabled();
    let handle = registry().get(pool_name).await?;
    let pool = handle.clone_pool();

    let start = Instant::now();
    let results = pool.query_columnar(sql, params, col_types).await?;
    let elapsed_us = start.elapsed().as_micros();

    if profile {
        eprintln!(
            "[OXYDE_PROFILE] execute_query_columnar ({}, {} rows): total={} µs",
            pool.backend_name(),
            results.1.len(),
            elapsed_us
        );
    }
    Ok(results)
}

pub async fn execute_statement(pool_name: &str, sql: &str, params: &[Value]) -> Result<u64> {
    debug!(
        "Executing statement on '{}': {} ({} params)",
        pool_name,
        sql,
        params.len()
    );

    let handle = registry().get(pool_name).await?;
    let pool = handle.clone_pool();
    let affected = pool.execute(sql, params).await?;

    debug!(
        "Statement on '{}' ({}) affected {} rows",
        pool_name,
        pool.backend_name(),
        affected
    );
    Ok(affected)
}

pub async fn execute_query_in_transaction(
    tx_id: u64,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
) -> Result<Vec<HashMap<String, serde_json::Value>>> {
    let registry = transaction_registry();
    let arc = registry
        .get(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut tx = arc.lock().await;
    if !tx.is_active() {
        return Err(DriverError::TransactionClosed(tx_id));
    }
    tx.update_activity();

    let conn = tx
        .conn
        .as_mut()
        .ok_or(DriverError::TransactionClosed(tx_id))?;
    conn.query(sql, params, col_types).await
}

/// Execute SELECT query in transaction returning columnar format (columns, rows).
pub async fn execute_query_columnar_in_transaction(
    tx_id: u64,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
) -> Result<ColumnarResult> {
    let registry = transaction_registry();
    let arc = registry
        .get(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut tx = arc.lock().await;
    if !tx.is_active() {
        return Err(DriverError::TransactionClosed(tx_id));
    }
    tx.update_activity();

    let conn = tx
        .conn
        .as_mut()
        .ok_or(DriverError::TransactionClosed(tx_id))?;
    conn.query_columnar(sql, params, col_types).await
}

pub async fn execute_statement_in_transaction(
    tx_id: u64,
    sql: &str,
    params: &[Value],
) -> Result<u64> {
    let registry = transaction_registry();
    let arc = registry
        .get(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut tx = arc.lock().await;
    if !tx.is_active() {
        return Err(DriverError::TransactionClosed(tx_id));
    }
    tx.update_activity();

    let conn = tx
        .conn
        .as_mut()
        .ok_or(DriverError::TransactionClosed(tx_id))?;
    conn.execute(sql, params).await
}

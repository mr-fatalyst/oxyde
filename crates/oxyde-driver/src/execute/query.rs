//! Query and statement execution (pool and transaction paths).

use sea_query::Value;
use std::collections::HashMap;
use std::time::Instant;
use tracing::debug;

use crate::convert::encoder::RelationInfo;
use crate::error::{DriverError, Result};
use crate::execute::traits::{ConnExec, PoolExec};
use crate::{registry, transaction_registry};

/// Check if profiling is enabled via OXYDE_PROFILE env var
fn is_profiling_enabled() -> bool {
    std::env::var("OXYDE_PROFILE")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Execute a SELECT query and return pre-encoded msgpack bytes + row count.
pub async fn execute_query_columnar(
    pool_name: &str,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
) -> Result<(Vec<u8>, usize)> {
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
            results.1,
            elapsed_us
        );
    }
    Ok(results)
}

/// Execute a SELECT with JOIN dedup encoding.
pub async fn execute_query_columnar_dedup(
    pool_name: &str,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
    relations: &[RelationInfo],
) -> Result<(Vec<u8>, usize)> {
    debug!(
        "Executing columnar dedup query on '{}': {} ({} params, {} relations)",
        pool_name,
        sql,
        params.len(),
        relations.len()
    );

    let handle = registry().get(pool_name).await?;
    let pool = handle.clone_pool();
    pool.query_columnar_dedup(sql, params, col_types, relations)
        .await
}

/// Execute a mutation with RETURNING clause, return pre-encoded msgpack map.
pub async fn execute_mutation_returning(
    pool_name: &str,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
) -> Result<Vec<u8>> {
    debug!(
        "Executing mutation+returning on '{}': {} ({} params)",
        pool_name,
        sql,
        params.len()
    );

    let handle = registry().get(pool_name).await?;
    let pool = handle.clone_pool();
    pool.query_mutation_returning(sql, params, col_types).await
}

/// Execute a statement (INSERT/UPDATE/DELETE without RETURNING) and return affected row count.
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

/// Execute SELECT query in transaction returning pre-encoded msgpack bytes + row count.
pub async fn execute_query_columnar_in_transaction(
    tx_id: u64,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
) -> Result<(Vec<u8>, usize)> {
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

/// Execute SELECT with JOIN dedup in transaction.
pub async fn execute_query_columnar_dedup_in_transaction(
    tx_id: u64,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
    relations: &[RelationInfo],
) -> Result<(Vec<u8>, usize)> {
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
    conn.query_columnar_dedup(sql, params, col_types, relations)
        .await
}

/// Execute mutation+RETURNING in transaction, return pre-encoded msgpack map.
pub async fn execute_mutation_returning_in_transaction(
    tx_id: u64,
    sql: &str,
    params: &[Value],
    col_types: Option<&HashMap<String, String>>,
) -> Result<Vec<u8>> {
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
    conn.query_mutation_returning(sql, params, col_types).await
}

/// Execute a statement within a transaction, return affected row count.
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

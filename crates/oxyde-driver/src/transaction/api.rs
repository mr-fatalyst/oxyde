//! Transaction lifecycle API: begin, commit, rollback, savepoints.

use tracing::info;

use crate::error::{DriverError, Result};
use crate::transaction::{begin_on_pool, TransactionInner};
use crate::{ensure_cleanup_task, registry, transaction_registry};

/// Begin a new transaction on the named pool, returns transaction ID.
pub async fn begin_transaction(pool_name: &str) -> Result<u64> {
    info!("Beginning transaction on pool '{}'", pool_name);
    let handle = registry().get(pool_name).await?;
    let now = std::time::Instant::now();

    let tx = begin_on_pool(&handle.clone_pool()).await?;

    let tx_inner = TransactionInner {
        pool_name: pool_name.to_string(),
        tx: Some(tx),
        created_at: now,
        last_activity: now,
    };

    let tx_id = transaction_registry().insert(tx_inner).await;

    // Ensure cleanup task is running
    ensure_cleanup_task();

    Ok(tx_id)
}

/// Commit a transaction and release the connection back to the pool.
pub async fn commit_transaction(tx_id: u64) -> Result<()> {
    info!("Committing transaction {}", tx_id);
    let registry = transaction_registry();
    let arc = registry
        .remove(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut inner = arc.lock().await;
    let tx = inner
        .tx
        .take()
        .ok_or(DriverError::TransactionClosed(tx_id))?;
    tx.commit().await
}

/// Rollback a transaction and release the connection back to the pool.
pub async fn rollback_transaction(tx_id: u64) -> Result<()> {
    info!("Rolling back transaction {}", tx_id);
    let registry = transaction_registry();
    let arc = registry
        .remove(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut inner = arc.lock().await;
    let tx = inner
        .tx
        .take()
        .ok_or(DriverError::TransactionClosed(tx_id))?;
    tx.rollback().await
}

/// Run a savepoint statement on an active transaction.
async fn savepoint_stmt(tx_id: u64, sql: &str, label: &str) -> Result<()> {
    let registry = transaction_registry();
    let arc = registry
        .get(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut inner = arc.lock().await;
    inner.update_activity();
    let tx = inner
        .tx
        .as_mut()
        .ok_or(DriverError::TransactionClosed(tx_id))?;
    tx.execute_raw(sql)
        .await
        .map_err(|e| DriverError::ExecutionError(format!("{label} failed: {e}")))
}

/// Create a named savepoint within a transaction.
pub async fn create_savepoint(tx_id: u64, savepoint_name: &str) -> Result<()> {
    info!(
        "Creating savepoint '{}' in transaction {}",
        savepoint_name, tx_id
    );
    let sql = format!("SAVEPOINT {savepoint_name}");
    savepoint_stmt(tx_id, &sql, "SAVEPOINT").await
}

/// Rollback to a named savepoint, undoing changes since the savepoint.
pub async fn rollback_to_savepoint(tx_id: u64, savepoint_name: &str) -> Result<()> {
    info!(
        "Rolling back to savepoint '{}' in transaction {}",
        savepoint_name, tx_id
    );
    let sql = format!("ROLLBACK TO SAVEPOINT {savepoint_name}");
    savepoint_stmt(tx_id, &sql, "ROLLBACK TO SAVEPOINT").await
}

/// Release a savepoint, making its changes permanent within the transaction.
pub async fn release_savepoint(tx_id: u64, savepoint_name: &str) -> Result<()> {
    info!(
        "Releasing savepoint '{}' in transaction {}",
        savepoint_name, tx_id
    );
    let sql = format!("RELEASE SAVEPOINT {savepoint_name}");
    savepoint_stmt(tx_id, &sql, "RELEASE SAVEPOINT").await
}

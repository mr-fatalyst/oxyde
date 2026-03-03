//! Transaction lifecycle API: begin, commit, rollback, savepoints.

use tracing::info;

use crate::error::{DriverError, Result};
use crate::transaction::{begin_on_pool, with_conn, TransactionInner, TransactionState};
use crate::{ensure_cleanup_task, registry, transaction_registry};

pub async fn begin_transaction(pool_name: &str) -> Result<u64> {
    info!("Beginning transaction on pool '{}'", pool_name);
    let handle = registry().get(pool_name).await?;
    let backend = handle.backend;
    let now = std::time::Instant::now();

    // Acquire connection and begin transaction
    let conn = begin_on_pool(&handle.clone_pool(), backend).await?;

    let tx_inner = TransactionInner {
        _pool_name: pool_name.to_string(),
        _backend: backend,
        conn: Some(conn),
        state: TransactionState::Active,
        created_at: now,
        last_activity: now,
    };

    let tx_id = transaction_registry().insert(tx_inner).await;

    // Ensure cleanup task is running
    ensure_cleanup_task();

    Ok(tx_id)
}

pub async fn commit_transaction(tx_id: u64) -> Result<()> {
    info!("Committing transaction {}", tx_id);
    let registry = transaction_registry();
    let arc = registry
        .remove(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut tx = arc.lock().await;
    if !tx.is_active() {
        return Err(DriverError::TransactionClosed(tx_id));
    }
    if let Some(conn) = tx.conn.as_mut() {
        with_conn!(conn, |c| {
            sqlx::query("COMMIT")
                .execute(c.as_mut())
                .await
                .map_err(|e| DriverError::ExecutionError(format!("COMMIT failed: {}", e)))
                .map(|_| ())?
        });
    }
    tx.state = TransactionState::Committed;
    tx.conn.take();
    Ok(())
}

pub async fn rollback_transaction(tx_id: u64) -> Result<()> {
    info!("Rolling back transaction {}", tx_id);
    let registry = transaction_registry();
    let arc = registry
        .remove(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut tx = arc.lock().await;
    if !tx.is_active() {
        return Err(DriverError::TransactionClosed(tx_id));
    }
    if let Some(conn) = tx.conn.as_mut() {
        with_conn!(conn, |c| {
            sqlx::query("ROLLBACK")
                .execute(c.as_mut())
                .await
                .map_err(|e| DriverError::ExecutionError(format!("ROLLBACK failed: {}", e)))
                .map(|_| ())?
        });
    }
    tx.state = TransactionState::RolledBack;
    tx.conn.take();
    Ok(())
}

pub async fn create_savepoint(tx_id: u64, savepoint_name: &str) -> Result<()> {
    info!(
        "Creating savepoint '{}' in transaction {}",
        savepoint_name, tx_id
    );
    let registry = transaction_registry();
    let arc = registry
        .get(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut tx = arc.lock().await;
    if !tx.is_active() {
        return Err(DriverError::TransactionClosed(tx_id));
    }
    if let Some(conn) = tx.conn.as_mut() {
        let sql = format!("SAVEPOINT {}", savepoint_name);
        with_conn!(conn, |c| {
            sqlx::query(&sql)
                .execute(c.as_mut())
                .await
                .map_err(|e| DriverError::ExecutionError(format!("SAVEPOINT failed: {}", e)))
                .map(|_| ())?
        });
    }
    Ok(())
}

pub async fn rollback_to_savepoint(tx_id: u64, savepoint_name: &str) -> Result<()> {
    info!(
        "Rolling back to savepoint '{}' in transaction {}",
        savepoint_name, tx_id
    );
    let registry = transaction_registry();
    let arc = registry
        .get(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut tx = arc.lock().await;
    if !tx.is_active() {
        return Err(DriverError::TransactionClosed(tx_id));
    }
    if let Some(conn) = tx.conn.as_mut() {
        let sql = format!("ROLLBACK TO SAVEPOINT {}", savepoint_name);
        with_conn!(conn, |c| {
            sqlx::query(&sql)
                .execute(c.as_mut())
                .await
                .map_err(|e| {
                    DriverError::ExecutionError(format!("ROLLBACK TO SAVEPOINT failed: {}", e))
                })
                .map(|_| ())?
        });
    }
    Ok(())
}

pub async fn release_savepoint(tx_id: u64, savepoint_name: &str) -> Result<()> {
    info!(
        "Releasing savepoint '{}' in transaction {}",
        savepoint_name, tx_id
    );
    let registry = transaction_registry();
    let arc = registry
        .get(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut tx = arc.lock().await;
    if !tx.is_active() {
        return Err(DriverError::TransactionClosed(tx_id));
    }
    if let Some(conn) = tx.conn.as_mut() {
        let sql = format!("RELEASE SAVEPOINT {}", savepoint_name);
        with_conn!(conn, |c| {
            sqlx::query(&sql)
                .execute(c.as_mut())
                .await
                .map_err(|e| {
                    DriverError::ExecutionError(format!("RELEASE SAVEPOINT failed: {}", e))
                })
                .map(|_| ())?
        });
    }
    Ok(())
}

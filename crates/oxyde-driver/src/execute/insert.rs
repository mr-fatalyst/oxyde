//! INSERT RETURNING execution (pool and transaction paths).

use sea_query::Value;
use tracing::{debug, warn};

use crate::bind::{bind_mysql, bind_postgres, bind_sqlite};
use crate::convert::{convert_pg_rows, convert_sqlite_rows};
use crate::error::{DriverError, Result};
use crate::pool::DbPool;
use crate::transaction::DbConn;
use crate::{registry, transaction_registry};

/// Execute INSERT and return generated IDs (supports any PK type: i64, UUID, String, etc.)
///
/// # Arguments
/// * `pool_name` - Database pool name
/// * `sql` - INSERT SQL statement
/// * `params` - Query parameters
/// * `pk_column` - Primary key column name (defaults to "id" if None)
pub async fn execute_insert_returning(
    pool_name: &str,
    sql: &str,
    params: &[Value],
    pk_column: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let pk_col = pk_column.unwrap_or("id");
    debug!(
        "Executing INSERT RETURNING on '{}': {} ({} params, pk={})",
        pool_name,
        sql,
        params.len(),
        pk_col
    );

    let handle = registry().get(pool_name).await?;
    match handle.clone_pool() {
        DbPool::Postgres(pool) => {
            // PostgreSQL: Use SQL as-is if it has RETURNING, else add RETURNING {pk_col}
            let has_returning = sql.to_uppercase().contains("RETURNING");
            let returning_sql = if has_returning {
                sql.to_string()
            } else {
                format!("{} RETURNING \"{}\"", sql, pk_col)
            };

            let query = bind_postgres(sqlx::query(&returning_sql), params)?;
            let rows = query.fetch_all(&pool).await.map_err(|e| {
                DriverError::ExecutionError(format!("INSERT RETURNING failed: {}", e))
            })?;

            // Extract PK values using batch row conversion
            let row_maps = convert_pg_rows(rows);
            let ids: Vec<serde_json::Value> = row_maps
                .into_iter()
                .filter_map(|row_map| row_map.get(pk_col).cloned().filter(|v| !v.is_null()))
                .collect();

            debug!(
                "INSERT on '{}' (Postgres) returned {} IDs",
                pool_name,
                ids.len()
            );
            Ok(ids)
        }
        DbPool::MySql(pool) => {
            // MySQL: Execute INSERT, then compute ID range from last_insert_id
            // Note: MySQL doesn't support RETURNING, so we can only get numeric auto-increment IDs
            let query = bind_mysql(sqlx::query(sql), params)?;
            let result = query
                .execute(&pool)
                .await
                .map_err(|e| DriverError::ExecutionError(format!("INSERT failed: {}", e)))?;

            let rows_affected = result.rows_affected() as i64;
            let last_id = result.last_insert_id() as i64;

            // MySQL last_insert_id() returns the FIRST auto-generated ID
            // Generate ID range: [first_id .. first_id + rows)
            let ids: Vec<serde_json::Value> = if rows_affected > 0 && last_id > 0 {
                (last_id..last_id + rows_affected)
                    .map(|id| serde_json::Value::Number(serde_json::Number::from(id)))
                    .collect()
            } else {
                vec![]
            };

            debug!(
                "INSERT on '{}' (MySQL) affected {} rows, last_id={}, generated {} IDs",
                pool_name,
                rows_affected,
                last_id,
                ids.len()
            );
            Ok(ids)
        }
        DbPool::Sqlite(pool) => {
            // SQLite: Try RETURNING first (SQLite 3.35+), fallback to last_insert_rowid
            let has_returning = sql.to_uppercase().contains("RETURNING");
            let returning_sql = if has_returning {
                sql.to_string()
            } else {
                format!("{} RETURNING \"{}\"", sql, pk_col)
            };

            let query = bind_sqlite(sqlx::query(&returning_sql), params)?;

            // Try RETURNING first
            match query.fetch_all(&pool).await {
                Ok(rows) => {
                    // RETURNING is supported - extract PK values using batch row conversion
                    let row_maps = convert_sqlite_rows(rows);
                    let ids: Vec<serde_json::Value> = row_maps
                        .into_iter()
                        .filter_map(|row_map| row_map.get(pk_col).cloned().filter(|v| !v.is_null()))
                        .collect();

                    debug!(
                        "INSERT on '{}' (SQLite) returned {} IDs via RETURNING",
                        pool_name,
                        ids.len()
                    );
                    Ok(ids)
                }
                Err(_) => {
                    // RETURNING not supported - fallback to last_insert_rowid (only works for INTEGER PK)
                    warn!(
                        "SQLite RETURNING not supported (version < 3.35), falling back to last_insert_rowid. \
                        This only works for INTEGER PRIMARY KEY and may produce incorrect IDs with concurrent inserts."
                    );

                    let query = bind_sqlite(sqlx::query(sql), params)?;
                    let result = query.execute(&pool).await.map_err(|e| {
                        DriverError::ExecutionError(format!("INSERT failed: {}", e))
                    })?;

                    let rows_affected = result.rows_affected() as i64;
                    let last_id = result.last_insert_rowid();

                    let ids: Vec<serde_json::Value> = if rows_affected > 0 && last_id > 0 {
                        (last_id - rows_affected + 1..=last_id)
                            .map(|id| serde_json::Value::Number(serde_json::Number::from(id)))
                            .collect()
                    } else {
                        vec![]
                    };

                    debug!(
                        "INSERT on '{}' (SQLite) affected {} rows, last_id={}, generated {} IDs via fallback",
                        pool_name, rows_affected, last_id, ids.len()
                    );
                    Ok(ids)
                }
            }
        }
    }
}

/// Execute INSERT within a transaction and return generated IDs (supports any PK type)
///
/// # Arguments
/// * `tx_id` - Transaction ID
/// * `sql` - INSERT SQL statement
/// * `params` - Query parameters
/// * `pk_column` - Primary key column name (defaults to "id" if None)
pub async fn execute_insert_returning_in_transaction(
    tx_id: u64,
    sql: &str,
    params: &[Value],
    pk_column: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let pk_col = pk_column.unwrap_or("id");
    let registry = transaction_registry();
    let arc = registry
        .get(tx_id)
        .await
        .ok_or(DriverError::TransactionNotFound(tx_id))?;
    let mut tx = arc.lock().await;
    if !tx.is_active() {
        return Err(DriverError::TransactionClosed(tx_id));
    }

    // Update activity timestamp to prevent premature cleanup
    tx.update_activity();

    let conn = tx
        .conn
        .as_mut()
        .ok_or(DriverError::TransactionClosed(tx_id))?;

    match conn {
        DbConn::Postgres(conn) => {
            // PostgreSQL: Use SQL as-is if it has RETURNING, else add RETURNING {pk_col}
            let has_returning = sql.to_uppercase().contains("RETURNING");
            let returning_sql = if has_returning {
                sql.to_string()
            } else {
                format!("{} RETURNING \"{}\"", sql, pk_col)
            };

            let query = bind_postgres(sqlx::query(&returning_sql), params)?;
            let rows = query.fetch_all(conn.as_mut()).await.map_err(|e| {
                DriverError::ExecutionError(format!("INSERT RETURNING failed: {}", e))
            })?;

            // Extract PK values using batch row conversion
            let row_maps = convert_pg_rows(rows);
            let ids: Vec<serde_json::Value> = row_maps
                .into_iter()
                .filter_map(|row_map| row_map.get(pk_col).cloned().filter(|v| !v.is_null()))
                .collect();

            debug!(
                "INSERT in transaction {} (Postgres) returned {} IDs",
                tx_id,
                ids.len()
            );
            Ok(ids)
        }
        DbConn::MySql(conn) => {
            // MySQL: Execute INSERT, then compute ID range from last_insert_id
            // Note: MySQL doesn't support RETURNING, so we can only get numeric auto-increment IDs
            let query = bind_mysql(sqlx::query(sql), params)?;
            let result = query
                .execute(conn.as_mut())
                .await
                .map_err(|e| DriverError::ExecutionError(format!("INSERT failed: {}", e)))?;

            let rows_affected = result.rows_affected() as i64;
            let last_id = result.last_insert_id() as i64;

            // MySQL last_insert_id() returns the FIRST auto-generated ID
            let ids: Vec<serde_json::Value> = if rows_affected > 0 && last_id > 0 {
                (last_id..last_id + rows_affected)
                    .map(|id| serde_json::Value::Number(serde_json::Number::from(id)))
                    .collect()
            } else {
                vec![]
            };

            debug!(
                "INSERT in transaction {} (MySQL) affected {} rows, first_id={}, generated {} IDs",
                tx_id,
                rows_affected,
                last_id,
                ids.len()
            );
            Ok(ids)
        }
        DbConn::Sqlite(conn) => {
            // SQLite: Try RETURNING first (SQLite 3.35+), fallback to last_insert_rowid
            let has_returning = sql.to_uppercase().contains("RETURNING");
            let returning_sql = if has_returning {
                sql.to_string()
            } else {
                format!("{} RETURNING \"{}\"", sql, pk_col)
            };

            let query = bind_sqlite(sqlx::query(&returning_sql), params)?;

            match query.fetch_all(conn.as_mut()).await {
                Ok(rows) => {
                    // RETURNING is supported - extract PK values using batch row conversion
                    let row_maps = convert_sqlite_rows(rows);
                    let ids: Vec<serde_json::Value> = row_maps
                        .into_iter()
                        .filter_map(|row_map| row_map.get(pk_col).cloned().filter(|v| !v.is_null()))
                        .collect();

                    debug!(
                        "INSERT in transaction {} (SQLite) returned {} IDs via RETURNING",
                        tx_id,
                        ids.len()
                    );
                    Ok(ids)
                }
                Err(_) => {
                    // RETURNING not supported - fallback to last_insert_rowid (only works for INTEGER PK)
                    warn!(
                        "SQLite RETURNING not supported (version < 3.35), falling back to last_insert_rowid. \
                        This only works for INTEGER PRIMARY KEY and may produce incorrect IDs with concurrent inserts."
                    );

                    let query = bind_sqlite(sqlx::query(sql), params)?;
                    let result = query.execute(conn.as_mut()).await.map_err(|e| {
                        DriverError::ExecutionError(format!("INSERT failed: {}", e))
                    })?;

                    let rows_affected = result.rows_affected() as i64;
                    let last_id = result.last_insert_rowid();

                    let ids: Vec<serde_json::Value> = if rows_affected > 0 && last_id > 0 {
                        (last_id - rows_affected + 1..=last_id)
                            .map(|id| serde_json::Value::Number(serde_json::Number::from(id)))
                            .collect()
                    } else {
                        vec![]
                    };

                    debug!(
                        "INSERT in transaction {} (SQLite) affected {} rows, last_id={}, generated {} IDs via fallback",
                        tx_id, rows_affected, last_id, ids.len()
                    );
                    Ok(ids)
                }
            }
        }
    }
}

//! INSERT RETURNING execution (pool and transaction paths).

use sea_query::Value;
use sqlx::Row;
use tracing::{debug, warn};

use crate::bind::{bind_mysql, bind_postgres, bind_sqlite};
use crate::error::{DriverError, Result};
use crate::pool::DbPool;
use crate::transaction::DbConn;
use crate::{registry, transaction_registry};

/// Extract PK value from a row by column name.
/// Tries i64 first, then String (covers UUID, text PKs).
fn extract_pk<R: Row>(row: &R, pk_col: &str) -> Option<rmpv::Value>
where
    for<'r> i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> &'r str: sqlx::ColumnIndex<R>,
{
    if let Ok(v) = row.try_get::<i64, _>(pk_col) {
        return Some(rmpv::Value::Integer(rmpv::Integer::from(v)));
    }
    if let Ok(v) = row.try_get::<String, _>(pk_col) {
        return Some(rmpv::Value::String(v.into()));
    }
    None
}

/// Execute INSERT and return generated IDs (supports any PK type: i64, UUID, String, etc.)
pub async fn execute_insert_returning(
    pool_name: &str,
    sql: &str,
    params: &[Value],
    pk_column: Option<&str>,
) -> Result<Vec<rmpv::Value>> {
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

            let ids: Vec<rmpv::Value> = rows
                .iter()
                .filter_map(|row| extract_pk(row, pk_col))
                .collect();

            debug!(
                "INSERT on '{}' (Postgres) returned {} IDs",
                pool_name,
                ids.len()
            );
            Ok(ids)
        }
        DbPool::MySql(pool) => {
            let query = bind_mysql(sqlx::query(sql), params)?;
            let result = query
                .execute(&pool)
                .await
                .map_err(|e| DriverError::ExecutionError(format!("INSERT failed: {}", e)))?;

            let rows_affected = result.rows_affected() as i64;
            let last_id = result.last_insert_id() as i64;

            let ids: Vec<rmpv::Value> = if rows_affected > 0 && last_id > 0 {
                (last_id..last_id + rows_affected)
                    .map(|id| rmpv::Value::Integer(rmpv::Integer::from(id)))
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
            let has_returning = sql.to_uppercase().contains("RETURNING");
            let returning_sql = if has_returning {
                sql.to_string()
            } else {
                format!("{} RETURNING \"{}\"", sql, pk_col)
            };

            let query = bind_sqlite(sqlx::query(&returning_sql), params)?;

            match query.fetch_all(&pool).await {
                Ok(rows) => {
                    let ids: Vec<rmpv::Value> = rows
                        .iter()
                        .filter_map(|row| extract_pk(row, pk_col))
                        .collect();

                    debug!(
                        "INSERT on '{}' (SQLite) returned {} IDs via RETURNING",
                        pool_name,
                        ids.len()
                    );
                    Ok(ids)
                }
                Err(_) => {
                    warn!(
                        "SQLite RETURNING not supported (version < 3.35), falling back to last_insert_rowid."
                    );

                    let query = bind_sqlite(sqlx::query(sql), params)?;
                    let result = query.execute(&pool).await.map_err(|e| {
                        DriverError::ExecutionError(format!("INSERT failed: {}", e))
                    })?;

                    let rows_affected = result.rows_affected() as i64;
                    let last_id = result.last_insert_rowid();

                    let ids: Vec<rmpv::Value> = if rows_affected > 0 && last_id > 0 {
                        (last_id - rows_affected + 1..=last_id)
                            .map(|id| rmpv::Value::Integer(rmpv::Integer::from(id)))
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
pub async fn execute_insert_returning_in_transaction(
    tx_id: u64,
    sql: &str,
    params: &[Value],
    pk_column: Option<&str>,
) -> Result<Vec<rmpv::Value>> {
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
    tx.update_activity();

    let conn = tx
        .conn
        .as_mut()
        .ok_or(DriverError::TransactionClosed(tx_id))?;

    match conn {
        DbConn::Postgres(conn) => {
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

            let ids: Vec<rmpv::Value> = rows
                .iter()
                .filter_map(|row| extract_pk(row, pk_col))
                .collect();

            debug!(
                "INSERT in transaction {} (Postgres) returned {} IDs",
                tx_id,
                ids.len()
            );
            Ok(ids)
        }
        DbConn::MySql(conn) => {
            let query = bind_mysql(sqlx::query(sql), params)?;
            let result = query
                .execute(conn.as_mut())
                .await
                .map_err(|e| DriverError::ExecutionError(format!("INSERT failed: {}", e)))?;

            let rows_affected = result.rows_affected() as i64;
            let last_id = result.last_insert_id() as i64;

            let ids: Vec<rmpv::Value> = if rows_affected > 0 && last_id > 0 {
                (last_id..last_id + rows_affected)
                    .map(|id| rmpv::Value::Integer(rmpv::Integer::from(id)))
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
            let has_returning = sql.to_uppercase().contains("RETURNING");
            let returning_sql = if has_returning {
                sql.to_string()
            } else {
                format!("{} RETURNING \"{}\"", sql, pk_col)
            };

            let query = bind_sqlite(sqlx::query(&returning_sql), params)?;

            match query.fetch_all(conn.as_mut()).await {
                Ok(rows) => {
                    let ids: Vec<rmpv::Value> = rows
                        .iter()
                        .filter_map(|row| extract_pk(row, pk_col))
                        .collect();

                    debug!(
                        "INSERT in transaction {} (SQLite) returned {} IDs via RETURNING",
                        tx_id,
                        ids.len()
                    );
                    Ok(ids)
                }
                Err(_) => {
                    warn!(
                        "SQLite RETURNING not supported (version < 3.35), falling back to last_insert_rowid."
                    );

                    let query = bind_sqlite(sqlx::query(sql), params)?;
                    let result = query.execute(conn.as_mut()).await.map_err(|e| {
                        DriverError::ExecutionError(format!("INSERT failed: {}", e))
                    })?;

                    let rows_affected = result.rows_affected() as i64;
                    let last_id = result.last_insert_rowid();

                    let ids: Vec<rmpv::Value> = if rows_affected > 0 && last_id > 0 {
                        (last_id - rows_affected + 1..=last_id)
                            .map(|id| rmpv::Value::Integer(rmpv::Integer::from(id)))
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

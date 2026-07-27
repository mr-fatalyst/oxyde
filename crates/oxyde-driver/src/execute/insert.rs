//! INSERT RETURNING execution (pool and transaction paths).

// rows_affected()/last_insert_id() are u64 by sqlx API; real values fit i64.
#![allow(clippy::cast_possible_wrap)]

use sea_query::Value;
use sqlx::Row;
use tracing::debug;

use crate::bind::{bind_mysql, bind_postgres, bind_sqlite};
use crate::error::{DriverError, Result};
use crate::pool::DbPool;
use crate::transaction::DbTx;
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
///
/// Postgres/SQLite: the builder already put pk-only `RETURNING "<pk>"` into
/// the SQL — PKs are read from the returned rows, the SQL text is never
/// modified here. MySQL has no RETURNING: PKs derive from `last_insert_id`.
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
            let query = bind_postgres(sqlx::query(sql), params)?;
            let rows = query
                .fetch_all(&pool)
                .await
                .map_err(|e| DriverError::db("INSERT RETURNING failed", e))?;

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
                .map_err(|e| DriverError::db("INSERT failed", e))?;

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
            let query = bind_sqlite(sqlx::query(sql), params)?;
            let rows = query
                .fetch_all(&pool)
                .await
                .map_err(|e| DriverError::db("INSERT RETURNING failed", e))?;

            let ids: Vec<rmpv::Value> = rows
                .iter()
                .filter_map(|row| extract_pk(row, pk_col))
                .collect();

            debug!(
                "INSERT on '{}' (SQLite) returned {} IDs",
                pool_name,
                ids.len()
            );
            Ok(ids)
        }
    }
}

/// Execute INSERT within a transaction and return generated IDs.
/// Same contract as [`execute_insert_returning`]: SQL arrives with pk-only
/// RETURNING already in place (Postgres/SQLite); MySQL uses `last_insert_id`.
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
        .tx
        .as_mut()
        .ok_or(DriverError::TransactionClosed(tx_id))?;

    match conn {
        DbTx::Postgres(tx) => {
            let query = bind_postgres(sqlx::query(sql), params)?;
            let rows = query
                .fetch_all(&mut **tx)
                .await
                .map_err(|e| DriverError::db("INSERT RETURNING failed", e))?;

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
        DbTx::MySql(tx) => {
            let query = bind_mysql(sqlx::query(sql), params)?;
            let result = query
                .execute(&mut **tx)
                .await
                .map_err(|e| DriverError::db("INSERT failed", e))?;

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
        DbTx::Sqlite(tx) => {
            let query = bind_sqlite(sqlx::query(sql), params)?;
            let rows = query
                .fetch_all(&mut **tx)
                .await
                .map_err(|e| DriverError::db("INSERT RETURNING failed", e))?;

            let ids: Vec<rmpv::Value> = rows
                .iter()
                .filter_map(|row| extract_pk(row, pk_col))
                .collect();

            debug!(
                "INSERT in transaction {} (SQLite) returned {} IDs",
                tx_id,
                ids.len()
            );
            Ok(ids)
        }
    }
}

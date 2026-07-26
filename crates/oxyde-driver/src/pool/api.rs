//! Pool lifecycle API: init, close, query backend.

use sqlx::{
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode},
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::path::PathBuf;
use std::str::FromStr;
use tracing::info;

use crate::error::{DriverError, Result};
use crate::pool::{DatabaseBackend, DbPool, PoolHandle};
use crate::settings::PoolSettings;
use crate::{registry, transaction_registry};

/// Initialize a named connection pool. Fails if pool with this name already exists.
pub async fn init_pool(name: &str, url: &str, settings: PoolSettings) -> Result<()> {
    init_pool_inner(name, url, settings, false).await
}

/// Initialize a named connection pool, replacing existing pool if present.
pub async fn init_pool_overwrite(name: &str, url: &str, settings: PoolSettings) -> Result<()> {
    init_pool_inner(name, url, settings, true).await
}

async fn init_pool_inner(
    name: &str,
    url: &str,
    settings: PoolSettings,
    overwrite: bool,
) -> Result<()> {
    info!(
        "Initializing pool '{}' with URL {} (overwrite={})",
        name, url, overwrite
    );

    validate_settings(&settings)?;

    let Some(backend) = backend_from_url(url) else {
        return Err(DriverError::ConnectionError(format!(
            "Unsupported database URL: {url}"
        )));
    };

    let pool = match backend {
        DatabaseBackend::Postgres => {
            let connect_opts = build_pg_connect_options(url, &settings)?;
            let pool_opts = apply_common_settings_pg(PgPoolOptions::new(), &settings);
            pool_opts
                .connect_with(connect_opts)
                .await
                .map(DbPool::Postgres)
                .map_err(|e| DriverError::ConnectionError(format!("Failed to connect: {e}")))?
        }
        DatabaseBackend::MySql => {
            let connect_opts = build_mysql_connect_options(url, &settings)?;
            let pool_opts = apply_common_settings_mysql(MySqlPoolOptions::new(), &settings);
            pool_opts
                .connect_with(connect_opts)
                .await
                .map(DbPool::MySql)
                .map_err(|e| DriverError::ConnectionError(format!("Failed to connect: {e}")))?
        }
        DatabaseBackend::Sqlite => {
            // Clone PRAGMA settings for use in after_connect closure
            let journal_mode_opt = settings.sqlite_journal_mode.clone();
            let synchronous_opt = settings.sqlite_synchronous.clone();
            let cache_size_opt = settings.sqlite_cache_size;
            let busy_timeout_opt = settings.sqlite_busy_timeout;

            let mut options = apply_common_settings_sqlite(SqlitePoolOptions::new(), &settings);

            // Apply PRAGMA settings to each connection in the pool
            options = options.after_connect(move |conn, _meta| {
                let journal_mode = journal_mode_opt.clone();
                let synchronous = synchronous_opt.clone();
                let cache_size = cache_size_opt;
                let busy_timeout = busy_timeout_opt;

                Box::pin(async move {
                    // journal_mode and synchronous are persistent (saved to DB file)
                    if let Some(mode) = journal_mode {
                        let pragma = format!("PRAGMA journal_mode = {mode}");
                        sqlx::query(&pragma).execute(&mut *conn).await?;
                    }

                    if let Some(sync) = synchronous {
                        let pragma = format!("PRAGMA synchronous = {sync}");
                        sqlx::query(&pragma).execute(&mut *conn).await?;
                    }

                    // cache_size and busy_timeout are per-connection (must be set for each connection)
                    if let Some(size) = cache_size {
                        let pragma = format!("PRAGMA cache_size = {size}");
                        sqlx::query(&pragma).execute(&mut *conn).await?;
                    }

                    if let Some(timeout) = busy_timeout {
                        let pragma = format!("PRAGMA busy_timeout = {timeout}");
                        sqlx::query(&pragma).execute(&mut *conn).await?;
                    }

                    Ok(())
                })
            });

            // Parse URL and enable automatic database file creation
            let connect_opts = SqliteConnectOptions::from_str(url)
                .map_err(|e| DriverError::ConnectionError(format!("Invalid SQLite URL: {e}")))?
                .create_if_missing(true);

            let pool = options
                .connect_with(connect_opts)
                .await
                .map_err(|e| DriverError::ConnectionError(format!("Failed to connect: {e}")))?;

            info!("SQLite pool created with PRAGMA settings (create_if_missing=true)");

            DbPool::Sqlite(pool)
        }
    };

    let handle = PoolHandle::new(backend, pool);

    if overwrite {
        // Close old pool if exists
        if let Some(old_handle) = registry().insert_or_replace(name.to_string(), handle).await {
            info!("Closing old pool '{}' during overwrite", name);
            old_handle.close().await;
        }
    } else if let Err(err) = registry().insert(name.to_string(), handle.clone()).await {
        handle.close().await;
        return Err(err);
    }

    // Update transaction registry settings for this pool
    transaction_registry()
        .update_settings(name, &settings)
        .await;

    info!("Pool '{}' initialised successfully", name);
    Ok(())
}

/// Close and remove a named connection pool.
pub async fn close_pool(name: &str) -> Result<()> {
    info!("Closing pool '{}'", name);
    let handle = registry().remove(name).await?;
    handle.close().await;
    Ok(())
}

/// Close all connection pools, rolling back any active transactions first.
pub async fn close_all_pools() -> Result<()> {
    info!("Closing all pools");

    // First, rollback all active transactions
    let rolled_back = transaction_registry().rollback_all().await;
    if rolled_back > 0 {
        info!(
            "Rolled back {} active transactions during shutdown",
            rolled_back
        );
    }

    // Then close all connection pools
    let handles = registry().take_all().await;
    for (name, handle) in handles {
        info!("Closing pool '{}'", name);
        handle.close().await;
    }
    Ok(())
}

/// Get the database backend type for a named pool.
pub async fn pool_backend(name: &str) -> Result<DatabaseBackend> {
    registry().get(name).await.map(|handle| handle.backend)
}

fn backend_from_url(url: &str) -> Option<DatabaseBackend> {
    if url.starts_with("postgres") {
        Some(DatabaseBackend::Postgres)
    } else if url.starts_with("mysql") {
        Some(DatabaseBackend::MySql)
    } else if url.starts_with("sqlite") {
        Some(DatabaseBackend::Sqlite)
    } else {
        None
    }
}

fn validate_settings(settings: &PoolSettings) -> Result<()> {
    if let (Some(min), Some(max)) = (settings.min_connections, settings.max_connections) {
        if min > max {
            return Err(DriverError::InvalidPoolSettings(
                "min_connections cannot exceed max_connections".into(),
            ));
        }
    }

    Ok(())
}

fn apply_common_settings_pg(mut options: PgPoolOptions, settings: &PoolSettings) -> PgPoolOptions {
    if let Some(max) = settings.max_connections {
        options = options.max_connections(max);
    }
    if let Some(min) = settings.min_connections {
        options = options.min_connections(min);
    }
    if let Some(timeout) = settings.acquire_timeout {
        options = options.acquire_timeout(timeout);
    }
    if let Some(timeout) = settings.idle_timeout {
        options = options.idle_timeout(timeout);
    }
    if let Some(lifetime) = settings.max_lifetime {
        options = options.max_lifetime(lifetime);
    }
    if let Some(test_before) = settings.test_before_acquire {
        options = options.test_before_acquire(test_before);
    }
    options
}

fn apply_common_settings_mysql(
    mut options: MySqlPoolOptions,
    settings: &PoolSettings,
) -> MySqlPoolOptions {
    if let Some(max) = settings.max_connections {
        options = options.max_connections(max);
    }
    if let Some(min) = settings.min_connections {
        options = options.min_connections(min);
    }
    if let Some(timeout) = settings.acquire_timeout {
        options = options.acquire_timeout(timeout);
    }
    if let Some(timeout) = settings.idle_timeout {
        options = options.idle_timeout(timeout);
    }
    if let Some(lifetime) = settings.max_lifetime {
        options = options.max_lifetime(lifetime);
    }
    if let Some(test_before) = settings.test_before_acquire {
        options = options.test_before_acquire(test_before);
    }
    options
}

fn apply_common_settings_sqlite(
    mut options: SqlitePoolOptions,
    settings: &PoolSettings,
) -> SqlitePoolOptions {
    if let Some(max) = settings.max_connections {
        options = options.max_connections(max);
    }
    if let Some(min) = settings.min_connections {
        options = options.min_connections(min);
    }
    if let Some(timeout) = settings.acquire_timeout {
        options = options.acquire_timeout(timeout);
    }
    if let Some(timeout) = settings.idle_timeout {
        options = options.idle_timeout(timeout);
    }
    if let Some(lifetime) = settings.max_lifetime {
        options = options.max_lifetime(lifetime);
    }
    if let Some(test_before) = settings.test_before_acquire {
        options = options.test_before_acquire(test_before);
    }
    options
}

fn build_pg_connect_options(url: &str, settings: &PoolSettings) -> Result<PgConnectOptions> {
    let mut opts = PgConnectOptions::from_str(url)
        .map_err(|e| DriverError::ConnectionError(format!("Invalid PostgreSQL URL: {e}")))?;

    if let Some(mode) = &settings.ssl_mode {
        opts = opts.ssl_mode(parse_pg_ssl_mode(mode)?);
    }
    if let Some(path) = &settings.ssl_root_cert {
        opts = opts.ssl_root_cert(PathBuf::from(path));
    }
    if let Some(path) = &settings.ssl_client_cert {
        opts = opts.ssl_client_cert(PathBuf::from(path));
    }
    if let Some(path) = &settings.ssl_client_key {
        opts = opts.ssl_client_key(PathBuf::from(path));
    }
    if let Some(name) = &settings.pg_application_name {
        opts = opts.application_name(name);
    }
    if let Some(cap) = settings.pg_statement_cache_capacity {
        opts = opts.statement_cache_capacity(cap as usize);
    }

    Ok(opts)
}

fn build_mysql_connect_options(url: &str, settings: &PoolSettings) -> Result<MySqlConnectOptions> {
    let mut opts = MySqlConnectOptions::from_str(url)
        .map_err(|e| DriverError::ConnectionError(format!("Invalid MySQL URL: {e}")))?;

    if let Some(mode) = &settings.ssl_mode {
        opts = opts.ssl_mode(parse_mysql_ssl_mode(mode)?);
    }
    if let Some(path) = &settings.ssl_root_cert {
        opts = opts.ssl_ca(PathBuf::from(path));
    }
    if let Some(path) = &settings.ssl_client_cert {
        opts = opts.ssl_client_cert(PathBuf::from(path));
    }
    if let Some(path) = &settings.ssl_client_key {
        opts = opts.ssl_client_key(PathBuf::from(path));
    }
    if let Some(charset) = &settings.mysql_charset {
        opts = opts.charset(charset);
    }
    if let Some(collation) = &settings.mysql_collation {
        opts = opts.collation(collation);
    }

    Ok(opts)
}

fn parse_pg_ssl_mode(mode: &str) -> Result<PgSslMode> {
    match mode.to_lowercase().replace('-', "_").as_str() {
        "disable" => Ok(PgSslMode::Disable),
        "allow" => Ok(PgSslMode::Allow),
        "prefer" => Ok(PgSslMode::Prefer),
        "require" => Ok(PgSslMode::Require),
        "verify_ca" => Ok(PgSslMode::VerifyCa),
        "verify_full" => Ok(PgSslMode::VerifyFull),
        _ => Err(DriverError::InvalidPoolSettings(format!(
            "Invalid PostgreSQL SSL mode: '{mode}'. \
             Expected: disable, allow, prefer, require, verify-ca, verify-full"
        ))),
    }
}

fn parse_mysql_ssl_mode(mode: &str) -> Result<MySqlSslMode> {
    match mode.to_lowercase().replace('-', "_").as_str() {
        "disabled" => Ok(MySqlSslMode::Disabled),
        "preferred" => Ok(MySqlSslMode::Preferred),
        "required" => Ok(MySqlSslMode::Required),
        "verify_ca" => Ok(MySqlSslMode::VerifyCa),
        "verify_identity" => Ok(MySqlSslMode::VerifyIdentity),
        _ => Err(DriverError::InvalidPoolSettings(format!(
            "Invalid MySQL SSL mode: '{mode}'. \
             Expected: disabled, preferred, required, verify-ca, verify-identity"
        ))),
    }
}

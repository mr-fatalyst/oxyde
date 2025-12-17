"""Python bindings to the Rust core (_oxyde_core extension module).

This module re-exports functions from the Rust PyO3 extension module and
provides the ir submodule for building query intermediate representations.

The Rust core provides:
    - Connection pool management (init_pool, close_pool)
    - Query execution (execute, execute_in_transaction)
    - Transaction control (begin/commit/rollback, savepoints)
    - Migration utilities (compute_diff, to_sql)

Architecture:
    Python Query → IR dict → msgpack bytes → Rust → SQL → Database
    Database → Rust → msgpack bytes → Python dict/models

Pool Functions:
    init_pool(name, url, settings): Create connection pool in Rust registry.
    init_pool_overwrite(name, url, settings): Replace existing pool.
    close_pool(name): Close specific pool.
    close_all_pools(): Close all pools (shutdown).

Execution Functions:
    execute(pool_name, ir_bytes): Execute query, return msgpack result.
    execute_in_transaction(pool_name, tx_id, ir_bytes): Execute in transaction.

Transaction Functions:
    begin_transaction(pool_name) → tx_id: Start transaction.
    commit_transaction(tx_id): Commit and release.
    rollback_transaction(tx_id): Rollback and release.
    create_savepoint(tx_id, name): Create savepoint for nested tx.
    rollback_to_savepoint(tx_id, name): Rollback to savepoint.
    release_savepoint(tx_id, name): Release savepoint.

Migration Functions:
    migration_compute_diff(models_schema, db_schema): Compute schema diff.
    migration_to_sql(diff, dialect): Generate SQL migration statements.

Validation:
    register_validator(model_key, validator): Register Pydantic validator.
    (Currently unused - validation happens in Python.)

Submodules:
    ir: Query IR construction helpers (build_select_ir, filter_and, etc.)
"""

from . import ir
from .wrapper import (
    begin_transaction,
    close_all_pools,
    close_pool,
    commit_transaction,
    create_savepoint,
    execute,
    execute_in_transaction,
    execute_select_batched,
    execute_select_batched_dedup,
    execute_select_direct,
    execute_to_pylist,
    explain_query,
    init_pool,
    init_pool_overwrite,
    migration_compute_diff,
    migration_to_sql,
    register_validator,
    release_savepoint,
    render_sql,
    render_sql_debug,
    rollback_to_savepoint,
    rollback_transaction,
)

__all__ = [
    "execute",
    "execute_in_transaction",
    "execute_select_direct",
    "execute_select_batched",
    "execute_select_batched_dedup",
    "execute_to_pylist",
    "init_pool",
    "init_pool_overwrite",
    "close_pool",
    "close_all_pools",
    "begin_transaction",
    "commit_transaction",
    "rollback_transaction",
    "create_savepoint",
    "rollback_to_savepoint",
    "release_savepoint",
    "register_validator",
    "render_sql",
    "render_sql_debug",
    "explain_query",
    "migration_compute_diff",
    "migration_to_sql",
    "ir",
]

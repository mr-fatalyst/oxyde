"""Migration executor - applies migrations to database."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Any

import msgpack

from oxyde.db.registry import get_connection as _get_connection_async
from oxyde.migrations.context import MigrationContext
from oxyde.migrations.replay import (
    SchemaState,
    _get_migration_dependency,
    _load_migration_module,
)
from oxyde.migrations.tracker import (
    ensure_migrations_table,
    get_applied_migrations,
    get_pending_migrations,
    record_migration,
    remove_migration,
)

if TYPE_CHECKING:
    from oxyde.db.pool import AsyncDatabase

# Advisory lock key for migrations (arbitrary unique number)
MIGRATION_LOCK_KEY = 0x4F587944  # "OxyD" in hex


def _parse_query_result(result_bytes: bytes) -> list[dict[str, Any]]:
    """Parse MessagePack query result into list of dicts.

    Args:
        result_bytes: Raw MessagePack bytes from query

    Returns:
        List of row dicts with column names as keys
    """
    if not result_bytes:
        return []

    result = msgpack.unpackb(result_bytes, raw=False)

    # Format: [columns, rows] where columns is list of names, rows is list of lists
    if isinstance(result, list) and len(result) == 2:
        columns, rows = result
        if isinstance(columns, list) and isinstance(rows, list):
            return [dict(zip(columns, row)) for row in rows]

    # Fallback: already list of dicts
    if isinstance(result, list) and all(isinstance(r, dict) for r in result):
        return result

    return []


def _check_migration_dependency(
    migration_path: Path,
    applied: set[str],
) -> None:
    """Check that migration's dependency is satisfied.

    Args:
        migration_path: Path to migration file
        applied: Set of already applied migration names

    Raises:
        RuntimeError: If dependency is not satisfied
    """
    module = _load_migration_module(migration_path)
    if module is None:
        return

    dependency = _get_migration_dependency(module)
    if dependency is not None and dependency not in applied:
        raise RuntimeError(
            f"Migration '{migration_path.stem}' depends on '{dependency}' "
            f"which has not been applied yet."
        )


async def _acquire_migration_lock(db_conn: AsyncDatabase, dialect: str) -> bool:
    """Acquire advisory lock to prevent concurrent migrations.

    Args:
        db_conn: Database connection
        dialect: Database dialect

    Returns:
        True if lock acquired, False if already locked by another process
    """
    from oxyde.core.ir import build_raw_sql_ir

    if dialect == "postgres":
        # PostgreSQL: pg_try_advisory_lock returns true if acquired
        sql = f"SELECT pg_try_advisory_lock({MIGRATION_LOCK_KEY})"
        ir = build_raw_sql_ir(sql=sql)
        result_bytes = await db_conn.execute(ir)
        result = _parse_query_result(result_bytes)
        if result and len(result) > 0:
            return result[0].get("pg_try_advisory_lock", False)
        return False

    elif dialect == "mysql":
        # MySQL: GET_LOCK returns 1 if acquired, 0 if timeout, NULL if error
        sql = "SELECT GET_LOCK('oxyde_migration', 0)"
        ir = build_raw_sql_ir(sql=sql)
        result_bytes = await db_conn.execute(ir)
        result = _parse_query_result(result_bytes)
        if result and len(result) > 0:
            val = list(result[0].values())[0]
            return val == 1
        return False

    elif dialect == "sqlite":
        # SQLite: File-level locking is automatic, no need for advisory lock
        # But we use PRAGMA to enable exclusive mode temporarily
        return True

    return True  # Unknown dialect - proceed without lock


async def _release_migration_lock(db_conn: AsyncDatabase, dialect: str) -> None:
    """Release advisory lock after migrations complete.

    Args:
        db_conn: Database connection
        dialect: Database dialect
    """
    from oxyde.core.ir import build_raw_sql_ir

    try:
        if dialect == "postgres":
            sql = f"SELECT pg_advisory_unlock({MIGRATION_LOCK_KEY})"
            ir = build_raw_sql_ir(sql=sql)
            await db_conn.execute(ir)

        elif dialect == "mysql":
            sql = "SELECT RELEASE_LOCK('oxyde_migration')"
            ir = build_raw_sql_ir(sql=sql)
            await db_conn.execute(ir)

        # SQLite: No explicit unlock needed

    except Exception:
        pass  # Ignore errors during unlock


def import_migration_module(filepath: Path) -> Any:
    """Import migration module from file.

    Args:
        filepath: Path to migration file

    Returns:
        Imported module
    """
    spec = importlib.util.spec_from_file_location(filepath.stem, filepath)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load migration from {filepath}")

    module = importlib.util.module_from_spec(spec)
    sys.modules[filepath.stem] = module
    spec.loader.exec_module(module)

    return module


def replay_migrations_up_to(
    migrations_dir: str,
    target_migration: Path,
    include_target: bool = False,
) -> SchemaState:
    """Replay migrations up to (but not including) target to get schema state.

    Args:
        migrations_dir: Path to migrations directory
        target_migration: Target migration path
        include_target: If True, include target migration in replay

    Returns:
        SchemaState after replaying migrations
    """
    state = SchemaState()
    migration_files = sorted(Path(migrations_dir).glob("[0-9]*.py"))

    for filepath in migration_files:
        # Stop before target (or at target if include_target is False)
        if filepath == target_migration and not include_target:
            break

        module = import_migration_module(filepath)

        if hasattr(module, "upgrade"):
            ctx = MigrationContext(mode="collect")
            module.upgrade(ctx)

            for op in ctx.get_collected_operations():
                state.apply_operation(op)

        # Stop after target if include_target is True
        if filepath == target_migration and include_target:
            break

    return state


async def apply_migrations(
    migrations_dir: str = "migrations",
    db_alias: str = "default",
    target: str | None = None,
    fake: bool = False,
) -> list[str]:
    """Apply pending migrations to database.

    Args:
        migrations_dir: Path to migrations directory
        db_alias: Database connection alias
        target: Target migration name (if None, apply all pending)
        fake: If True, mark as applied without executing SQL

    Returns:
        List of applied migration names
    """
    # Ensure migrations table exists
    await ensure_migrations_table(db_alias)

    # Get applied and pending migrations
    applied = await get_applied_migrations(db_alias)
    pending = get_pending_migrations(migrations_dir, applied)

    if not pending:
        return []

    # Filter by target if specified
    if target:
        target_index = None
        for i, migration_path in enumerate(pending):
            # Match by exact name or prefix (e.g. "0001" matches "0001_create_users_table")
            if migration_path.stem == target or migration_path.stem.startswith(
                target + "_"
            ):
                target_index = i
                break

        if target_index is None:
            raise ValueError(
                f"Target migration '{target}' not found in pending migrations"
            )

        pending = pending[: target_index + 1]

    # Get database connection and detect dialect
    db_conn = await _get_connection_async(db_alias)

    # Detect dialect from database URL
    url_lower = db_conn.url.lower()
    if url_lower.startswith("sqlite"):
        dialect = "sqlite"
    elif url_lower.startswith("postgres"):
        dialect = "postgres"
    elif url_lower.startswith("mysql"):
        dialect = "mysql"
    else:
        dialect = "sqlite"  # Default to sqlite

    # Acquire advisory lock to prevent concurrent migrations
    lock_acquired = await _acquire_migration_lock(db_conn, dialect)
    if not lock_acquired:
        raise RuntimeError(
            "Cannot acquire migration lock. Another migration process may be running."
        )

    # Apply each migration
    applied_migrations = []
    applied_set = set(applied)  # Track applied migrations for dependency check

    try:
        # Track schema state as we apply migrations
        schema_state = replay_migrations_up_to(
            migrations_dir, pending[0], include_target=False
        )

        for migration_path in pending:
            migration_name = migration_path.stem

            # Check dependency is satisfied
            _check_migration_dependency(migration_path, applied_set)

            if not fake:
                # Import migration module
                module = import_migration_module(migration_path)

                if not hasattr(module, "upgrade"):
                    raise RuntimeError(
                        f"Migration {migration_name} missing upgrade() function"
                    )

                # Create migration context in execute mode with schema state
                ctx = MigrationContext(
                    mode="execute",
                    dialect=dialect,
                    db_conn=db_conn,
                    schema_state=schema_state,
                )

                # Execute upgrade (collects SQL statements)
                module.upgrade(ctx)

                # Execute collected SQL (wrapped in transaction for postgres/sqlite)
                await ctx._execute_collected_sql()

                # Update schema state with this migration's operations
                ctx_collect = MigrationContext(mode="collect")
                module.upgrade(ctx_collect)
                for op in ctx_collect.get_collected_operations():
                    schema_state.apply_operation(op)

            # Record migration as applied
            await record_migration(migration_name, db_alias)
            applied_migrations.append(migration_name)
            applied_set.add(migration_name)  # Update for dependency checking

    finally:
        # Always release the lock
        await _release_migration_lock(db_conn, dialect)

    return applied_migrations


def _check_rollback_dependency(
    migration_name: str,
    migrations_dir: str,
    applied: list[str],
) -> None:
    """Check that no applied migration depends on the one being rolled back.

    Args:
        migration_name: Name of migration to roll back
        migrations_dir: Path to migrations directory
        applied: List of currently applied migration names

    Raises:
        RuntimeError: If another applied migration depends on this one
    """
    migrations_path = Path(migrations_dir)
    for applied_name in applied:
        if applied_name == migration_name:
            continue
        migration_path = migrations_path / f"{applied_name}.py"
        if not migration_path.exists():
            continue

        module = _load_migration_module(migration_path)
        if module is None:
            continue

        dependency = _get_migration_dependency(module)
        if dependency == migration_name:
            msg = (
                f"Cannot roll back '{migration_name}': "
                f"migration '{applied_name}' depends on it."
            )
            raise RuntimeError(msg)


async def rollback_migration(
    migration_name: str,
    migrations_dir: str = "migrations",
    db_alias: str = "default",
    fake: bool = False,
) -> None:
    """Roll back a single migration.

    Args:
        migration_name: Name of migration to roll back
        migrations_dir: Path to migrations directory
        db_alias: Database connection alias
        fake: If True, remove from history without executing SQL
    """
    migration_path = Path(migrations_dir) / f"{migration_name}.py"
    if not migration_path.exists():
        raise FileNotFoundError(f"Migration file not found: {migration_path}")

    # Check that no applied migration depends on this one
    applied = await get_applied_migrations(db_alias)
    _check_rollback_dependency(migration_name, migrations_dir, applied)

    # Get database connection and detect dialect
    db_conn = await _get_connection_async(db_alias)

    # Detect dialect from database URL
    url_lower = db_conn.url.lower()
    if url_lower.startswith("sqlite"):
        dialect = "sqlite"
    elif url_lower.startswith("postgres"):
        dialect = "postgres"
    elif url_lower.startswith("mysql"):
        dialect = "mysql"
    else:
        dialect = "sqlite"  # Default to sqlite

    # Acquire advisory lock
    lock_acquired = await _acquire_migration_lock(db_conn, dialect)
    if not lock_acquired:
        raise RuntimeError(
            "Cannot acquire migration lock. Another migration process may be running."
        )

    try:
        if not fake:
            # Import migration module
            module = import_migration_module(migration_path)

            if not hasattr(module, "downgrade"):
                raise RuntimeError(
                    f"Migration {migration_name} missing downgrade() function"
                )

            # Replay migrations up to and including this one to get schema state
            schema_state = replay_migrations_up_to(
                migrations_dir, migration_path, include_target=True
            )

            # Create migration context in execute mode with schema state
            ctx = MigrationContext(
                mode="execute",
                dialect=dialect,
                db_conn=db_conn,
                schema_state=schema_state,
            )

            # Execute downgrade (collects SQL statements)
            module.downgrade(ctx)

            # Execute collected SQL
            await ctx._execute_collected_sql()

        # Remove migration from history
        await remove_migration(migration_name, db_alias)

    finally:
        await _release_migration_lock(db_conn, dialect)


async def rollback_migrations(
    steps: int = 1,
    migrations_dir: str = "migrations",
    db_alias: str = "default",
    fake: bool = False,
) -> list[str]:
    """Roll back last N migrations.

    Args:
        steps: Number of migrations to roll back
        migrations_dir: Path to migrations directory
        db_alias: Database connection alias
        fake: If True, remove from history without executing SQL

    Returns:
        List of rolled back migration names
    """
    # Get applied migrations
    applied = await get_applied_migrations(db_alias)

    if not applied:
        return []

    # Get last N migrations to roll back
    to_rollback = applied[-steps:] if steps < len(applied) else applied
    to_rollback = list(reversed(to_rollback))  # Roll back in reverse order

    rolled_back = []
    for migration_name in to_rollback:
        await rollback_migration(migration_name, migrations_dir, db_alias, fake)
        rolled_back.append(migration_name)

    return rolled_back


__all__ = [
    "apply_migrations",
    "rollback_migration",
    "rollback_migrations",
]

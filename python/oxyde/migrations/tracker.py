"""Migration history tracking."""

from __future__ import annotations

from datetime import datetime
from pathlib import Path

from oxyde.db.registry import get_connection as _get_connection_async

MIGRATIONS_TABLE = "oxyde_migrations"


async def ensure_migrations_table(db_alias: str = "default") -> None:
    """Create migrations tracking table if it doesn't exist.

    Args:
        db_alias: Database connection alias
    """
    db_conn = await _get_connection_async(db_alias)

    # Check if table exists (works for all databases)
    create_sql = f"""
    CREATE TABLE IF NOT EXISTS {MIGRATIONS_TABLE} (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE,
        applied_at TIMESTAMP NOT NULL
    )
    """

    # Execute using IR
    from oxyde.core.ir import build_raw_sql_ir

    create_ir = build_raw_sql_ir(sql=create_sql)
    await db_conn.execute(create_ir)


async def get_applied_migrations(db_alias: str = "default") -> list[str]:
    """Get list of applied migration names.

    Args:
        db_alias: Database connection alias

    Returns:
        List of applied migration names (sorted by applied_at)
    """
    await ensure_migrations_table(db_alias)

    db_conn = await _get_connection_async(db_alias)

    # Query applied migrations
    query_sql = f"""
    SELECT name FROM {MIGRATIONS_TABLE}
    ORDER BY id ASC
    """

    from oxyde.core.ir import build_raw_sql_ir

    query_ir = build_raw_sql_ir(sql=query_sql)
    result_bytes = await db_conn.execute(query_ir)

    # Result comes as MessagePack bytes (from serialize_results)
    import msgpack

    result = msgpack.unpackb(result_bytes, raw=False)

    # Extract migration names from results
    migrations = []
    for row in result:
        # row is a dict with 'name' key
        if isinstance(row, dict):
            migrations.append(row["name"])
        else:
            # In case it's a tuple/list
            migrations.append(row[0])

    return migrations


async def record_migration(name: str, db_alias: str = "default") -> None:
    """Record that a migration has been applied.

    Args:
        name: Migration name (e.g., "0001_initial")
        db_alias: Database connection alias
    """
    await ensure_migrations_table(db_alias)

    db_conn = await _get_connection_async(db_alias)

    # Insert migration record
    insert_sql = f"""
    INSERT INTO {MIGRATIONS_TABLE} (name, applied_at)
    VALUES (?, ?)
    """

    from oxyde.core.ir import build_raw_sql_ir

    insert_ir = build_raw_sql_ir(
        sql=insert_sql,
        params=[name, datetime.now().isoformat()],
    )
    await db_conn.execute(insert_ir)


async def remove_migration(name: str, db_alias: str = "default") -> None:
    """Remove a migration record (for rollback).

    Args:
        name: Migration name to remove
        db_alias: Database connection alias
    """
    db_conn = await _get_connection_async(db_alias)

    delete_sql = f"""
    DELETE FROM {MIGRATIONS_TABLE}
    WHERE name = ?
    """

    from oxyde.core.ir import build_raw_sql_ir

    delete_ir = build_raw_sql_ir(sql=delete_sql, params=[name])
    await db_conn.execute(delete_ir)


def get_migration_files(migrations_dir: str = "migrations") -> list[Path]:
    """Get list of migration files sorted by number.

    Args:
        migrations_dir: Path to migrations directory

    Returns:
        List of migration file paths
    """
    migrations_path = Path(migrations_dir)
    if not migrations_path.exists():
        return []

    # Find all migration files (0001_*.py, 0002_*.py, etc.)
    migration_files = sorted(migrations_path.glob("[0-9]*.py"))
    return migration_files


def get_pending_migrations(
    migrations_dir: str = "migrations",
    applied: list[str] | None = None,
) -> list[Path]:
    """Get list of pending (unapplied) migrations.

    Args:
        migrations_dir: Path to migrations directory
        applied: List of applied migration names (if None, will query database)

    Returns:
        List of pending migration file paths
    """
    all_migrations = get_migration_files(migrations_dir)
    applied_set = set(applied or [])

    pending = []
    for filepath in all_migrations:
        # Extract migration name (0001_initial.py -> 0001_initial)
        name = filepath.stem
        if name not in applied_set:
            pending.append(filepath)

    return pending


__all__ = [
    "ensure_migrations_table",
    "get_applied_migrations",
    "record_migration",
    "remove_migration",
    "get_migration_files",
    "get_pending_migrations",
]

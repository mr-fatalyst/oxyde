"""Schema management. create/drop tables from registered models."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any

from oxyde.core import migration_compute_diff, migration_to_sql
from oxyde.migrations.extract import extract_current_schema
from oxyde.migrations.utils import detect_dialect
from oxyde.queries.raw import execute_raw

if TYPE_CHECKING:
    from oxyde.db.pool import AsyncDatabase


async def create_tables(database: AsyncDatabase) -> None:
    """Create all tables for registered models.

    Generates dialect-aware DDL via sea-query and executes it.
    No migration files or tracking table needed.

    Args:
        database: Connected AsyncDatabase instance.
    """
    dialect = detect_dialect(database.url)
    current = extract_current_schema(dialect=dialect)
    empty: dict[str, Any] = {"version": 1, "tables": {}}
    ops_json = migration_compute_diff(json.dumps(empty), json.dumps(current))
    statements = migration_to_sql(ops_json, dialect)
    for sql in statements:
        await execute_raw(sql, client=database)


async def drop_tables(database: AsyncDatabase) -> None:
    """Drop all tables for registered models.

    Handles FK constraints per dialect.

    Args:
        database: Connected AsyncDatabase instance.
    """
    dialect = detect_dialect(database.url)
    current = extract_current_schema(dialect=dialect)
    table_names = list(current.get("tables", {}).keys())

    if not table_names:
        return

    if dialect == "postgres":
        tables = ", ".join(table_names)
        await execute_raw(f"DROP TABLE IF EXISTS {tables} CASCADE", client=database)
    elif dialect == "mysql":
        await execute_raw("SET FOREIGN_KEY_CHECKS = 0", client=database)
        for t in table_names:
            await execute_raw(f"DROP TABLE IF EXISTS {t}", client=database)
        await execute_raw("SET FOREIGN_KEY_CHECKS = 1", client=database)
    else:
        # SQLite: typically fresh file per test, but support drop anyway
        for t in table_names:
            await execute_raw(f"DROP TABLE IF EXISTS {t}", client=database)

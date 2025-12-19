"""Raw SQL execution with automatic connection resolution.

This module provides execute_raw() - a function for executing arbitrary SQL
while respecting the same connection resolution logic as Model.objects.

Connection Resolution:
    1. Explicit client parameter (AsyncDatabase or AsyncTransaction)
    2. Active transaction (if inside atomic())
    3. Named connection (using="alias")
    4. Default connection ("default")

Example:
    from oxyde import execute_raw

    # Simple query (uses default connection)
    users = await execute_raw("SELECT * FROM users WHERE age > $1", [18])

    # Inside transaction - automatically uses same transaction
    async with transaction.atomic():
        await User.objects.create(name="Alice")
        await execute_raw("INSERT INTO audit_log VALUES ($1, $2)", [1, "created"])

    # Specific connection
    stats = await execute_raw("SELECT ...", using="analytics")
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import msgpack

from oxyde.queries.base import _resolve_execution_client

if TYPE_CHECKING:
    from oxyde.queries.base import SupportsExecute


async def execute_raw(
    sql: str,
    params: list[Any] | None = None,
    *,
    using: str | None = None,
    client: SupportsExecute | None = None,
) -> list[dict[str, Any]]:
    """
    Execute raw SQL query.

    Uses the same connection resolution as Model.objects:
    - Active transaction (if inside atomic())
    - Named connection (using="alias")
    - Default connection

    Args:
        sql: SQL with placeholders ($1/$2 for Postgres, ? for SQLite/MySQL)
        params: Query parameters (use these to prevent SQL injection!)
        using: Connection alias (default: "default")
        client: Explicit client (AsyncDatabase or AsyncTransaction)

    Returns:
        List of dicts for SELECT queries.
        Empty list for INSERT/UPDATE/DELETE without RETURNING.

    Raises:
        RuntimeError: If no connection is available.
        ManagerError: If both 'using' and 'client' are provided.

    Example:
        # Simple SELECT
        users = await execute_raw("SELECT * FROM users WHERE age > $1", [18])

        # Inside transaction (automatically uses same tx)
        async with transaction.atomic():
            await User.objects.create(name="Alice")
            await execute_raw(
                "INSERT INTO audit_log (user_id, action) VALUES ($1, $2)",
                [1, "created"]
            )

        # PostgreSQL-specific features
        results = await execute_raw(
            "SELECT * FROM products WHERE metadata @> $1::jsonb",
            ['{"featured": true}']
        )

        # Different connection
        stats = await execute_raw("SELECT ...", using="analytics")

    Warning:
        Always use parameterized queries! Never interpolate user input:

        # GOOD
        await execute_raw("SELECT * FROM users WHERE email = $1", [email])

        # BAD - SQL injection risk!
        await execute_raw(f"SELECT * FROM users WHERE email = '{email}'")
    """
    execution_client = await _resolve_execution_client(using, client)

    ir = {
        "proto": 1,
        "op": "raw",
        "table": "",
        "sql": sql,
        "params": params or [],
    }

    result_bytes = await execution_client.execute(ir)

    # Columnar format from Rust: (columns, rows) -> list[dict]
    columns, rows = msgpack.unpackb(result_bytes, raw=False)
    return [dict(zip(columns, row)) for row in rows]


__all__ = ["execute_raw"]

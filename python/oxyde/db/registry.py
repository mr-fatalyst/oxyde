"""Global registry for named database connections.

This module maintains a dict mapping connection names to AsyncDatabase instances.
The "default" connection is used when no explicit name is specified.

Why a Registry?
    Queries need database connections, but models don't store connection refs.
    The registry provides a global lookup: Model.objects uses get_connection()
    to find the right pool.

    # At startup
    await db.init(default="postgresql://...", analytics="postgresql://...")

    # In queries (implicit lookup via registry)
    users = await User.objects.all()  # Uses "default"
    stats = await Metric.objects.all(using="analytics")  # Uses "analytics"

Functions:
    register_connection(database, overwrite=False):
        Add AsyncDatabase to registry. Raises if name exists and not overwrite.

    unregister_connection(name):
        Remove from registry (used during cleanup).

    get_connection(name="default", ensure_connected=True):
        Look up by name. Auto-connects if ensure_connected=True.
        Raises KeyError if not found.

    disconnect_all():
        Close all pools (both Python registry and Rust pools).
        Clears the registry. Used at shutdown.

Thread Safety:
    The registry is a simple dict. In async context, this is safe because
    Python's GIL prevents concurrent dict mutations. Registration typically
    happens at startup before concurrent access.

Transaction Integration:
    QueryManager._resolve_client() checks get_active_transaction() before
    falling back to get_connection(). This ensures queries inside atomic()
    use the transaction, not a new pool connection.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from oxyde.db.pool import AsyncDatabase

# Global connection registry
_CONNECTIONS: dict[str, AsyncDatabase] = {}


def register_connection(
    database: AsyncDatabase, *, overwrite: bool = False
) -> AsyncDatabase:
    """Register a connection instance so it can be retrieved later."""
    if not overwrite and database.name in _CONNECTIONS:
        raise ValueError(f"Connection '{database.name}' already registered")
    _CONNECTIONS[database.name] = database
    return database


def unregister_connection(name: str) -> None:
    """Remove a connection from the registry."""
    _CONNECTIONS.pop(name, None)


async def get_connection(
    name: str = "default",
    *,
    ensure_connected: bool = True,
) -> AsyncDatabase:
    """
    Retrieve a registered connection by name.

    Args:
        name: Connection identifier.
        ensure_connected: Automatically call connect() if the pool is not ready.
    """
    try:
        database = _CONNECTIONS[name]
    except KeyError as exc:
        raise KeyError(f"Connection '{name}' is not registered") from exc

    if ensure_connected and not database.connected:
        await database.connect()

    return database


async def disconnect_all() -> None:
    """Disconnect all registered pools and clear the registry."""
    from oxyde.db.pool import close_all_pools

    # Mark all Python-side connections as disconnected
    for db in _CONNECTIONS.values():
        db._connected = False

    # Clear Python registry
    _CONNECTIONS.clear()

    # Close all Rust pools (also rolls back active transactions)
    await close_all_pools()


__all__ = [
    "register_connection",
    "unregister_connection",
    "get_connection",
    "disconnect_all",
]

"""Database connection management and transaction support.

This module provides the primary interface for database connectivity in Oxyde.
It wraps the Rust connection pool and provides Python-friendly APIs.

High-Level Functions:
    init(**databases, settings=None):
        Initialize one or more database connections at startup.
        Example: await db.init(default="postgresql://...")

    close():
        Disconnect all database connections.
        Example: await db.close()

    connect(url, name="default"):
        Context manager for single database connection.
        Example: async with db.connect("sqlite://test.db"): ...

    lifespan(**databases):
        FastAPI/Starlette lifespan context manager.
        Example: app = FastAPI(lifespan=db.lifespan(default="..."))

Classes:
    AsyncDatabase: Connection pool wrapper with execute() method.
    AsyncTransaction: Transaction wrapper with execute() method.
    PoolSettings: Configuration for connection pool and SQLite PRAGMAs.

Transaction API (Django-style):
    from oxyde.db import transaction

    async with transaction.atomic():
        await User.objects.create(name="Alice")
        # Auto-commits on success, rolls back on exception

    Nested transactions use savepoints:
    async with transaction.atomic():
        await User.objects.create(name="Alice")
        async with transaction.atomic():
            await User.objects.create(name="Bob")
            # Inner failure rolls back to savepoint, not entire transaction

Registry Functions:
    register_connection(): Add connection to global registry.
    get_connection(): Get connection by name (default: "default").
    disconnect_all(): Close all connections.

Connection Resolution:
    Queries automatically use the active transaction if inside atomic(),
    otherwise they use the connection from the registry.
"""

from __future__ import annotations

from collections.abc import AsyncIterator, Callable
from contextlib import AbstractAsyncContextManager, asynccontextmanager
from typing import Any

from . import transaction
from .pool import AsyncDatabase, PoolSettings
from .registry import disconnect_all, get_connection, register_connection
from .transaction import (
    AsyncTransaction,
    TransactionTimeoutError,
    atomic,
    get_active_transaction,
)


async def init(
    *,
    settings: PoolSettings | None = None,
    **databases: str,
) -> None:
    """
    Initialize database connections.

    Args:
        settings: Default pool settings for all connections.
        **databases: Mapping of name -> URL (e.g., default="postgresql://...")

    Raises:
        ValueError: If no databases provided.
        Exception: If any database fails to connect (all connections are rolled back).

    Example:
        await db.init(
            default="postgresql://user:pass@localhost/mydb",
            analytics="postgresql://user:pass@localhost/analytics",
            settings=PoolSettings(max_connections=20),
        )
    """
    if not databases:
        raise ValueError(
            "At least one database URL required (e.g., default='postgresql://...')"
        )

    connected: list[AsyncDatabase] = []
    try:
        for name, url in databases.items():
            database = AsyncDatabase(
                url,
                name=name,
                settings=settings,
                auto_register=True,
                overwrite=True,
            )
            await database.connect()
            connected.append(database)
    except Exception:
        # Rollback all successfully connected databases
        for db in connected:
            try:
                await db.disconnect()
            except Exception:
                pass  # Ignore errors during cleanup
        raise


async def close() -> None:
    """
    Close all database connections.

    Example:
        await db.close()
    """
    await disconnect_all()


@asynccontextmanager
async def connect(
    url: str,
    *,
    name: str = "default",
    settings: PoolSettings | None = None,
) -> AsyncIterator[AsyncDatabase]:
    """
    Context manager for a single database connection.

    Useful for scripts and tests where you want automatic cleanup.

    Args:
        url: Database connection URL.
        name: Connection name (default: "default").
        settings: Optional pool settings.

    Example:
        async with db.connect("sqlite://test.db") as conn:
            users = await User.objects.all()
    """
    from .registry import unregister_connection

    database = AsyncDatabase(
        url,
        name=name,
        settings=settings,
        auto_register=True,
        overwrite=True,
    )
    try:
        await database.connect()
        yield database
    finally:
        await database.disconnect()
        unregister_connection(name)


def lifespan(
    *,
    settings: PoolSettings | None = None,
    **databases: str,
) -> Callable[[Any], AbstractAsyncContextManager[None]]:
    """
    Create a lifespan context manager for FastAPI/Starlette.

    Args:
        settings: Default pool settings for all connections.
        **databases: Mapping of name -> URL.

    Returns:
        Async context manager compatible with FastAPI's lifespan parameter.

    Example:
        from fastapi import FastAPI
        from oxyde import db

        app = FastAPI(lifespan=db.lifespan(
            default="postgresql://user:pass@localhost/mydb",
        ))
    """

    @asynccontextmanager
    async def _lifespan(app: Any) -> AsyncIterator[None]:
        await init(settings=settings, **databases)
        try:
            yield
        finally:
            await close()

    return _lifespan


__all__ = [
    # High-level API
    "init",
    "close",
    "connect",
    "lifespan",
    "get_connection",
    # Classes
    "AsyncDatabase",
    "AsyncTransaction",
    "PoolSettings",
    "TransactionTimeoutError",
    # Transactions (Django-style: transaction.atomic())
    "transaction",
    "atomic",
    "get_active_transaction",
    # Low-level (for advanced use)
    "register_connection",
    "disconnect_all",
]

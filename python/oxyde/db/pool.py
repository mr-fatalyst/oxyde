"""Connection pool wrapper and configuration.

This module provides AsyncDatabase - the Python wrapper around the Rust
connection pool managed by sqlx. It implements the SupportsExecute protocol.

AsyncDatabase:
    Wraps a Rust connection pool identified by name. The actual pool lives
    in Rust; this class provides Python async interface.

    Methods:
        connect(): Initialize Rust pool with URL and settings.
        disconnect(): Close Rust pool.
        execute(ir): Send query IR to Rust, get MessagePack bytes back.

    Usage:
        db = AsyncDatabase("postgresql://...", name="default")
        await db.connect()
        result_bytes = await db.execute({"type": "select", ...})
        await db.disconnect()

PoolSettings:
    Configuration dataclass for pool tuning.

    Pool settings (all databases):
        max_connections: Maximum pool size (default: auto)
        min_connections: Minimum idle connections
        acquire_timeout: Max wait time for connection
        idle_timeout: Close idle connections after
        max_lifetime: Max connection age
        test_before_acquire: Ping before using connection

    Transaction settings:
        transaction_timeout: Max transaction duration (default: 5 min)
        transaction_cleanup_interval: Cleanup check interval (default: 1 min)

    SQLite-specific PRAGMAs (auto-applied on connect):
        sqlite_journal_mode: "WAL" (default, 10-20x faster writes)
        sqlite_synchronous: "NORMAL" (default, balance speed/safety)
        sqlite_cache_size: 10000 pages (~10MB)
        sqlite_busy_timeout: 5000ms lock wait timeout

Rust Integration:
    _init_pool(): Create pool in Rust registry
    _execute(): Send IR bytes to Rust, execute via sqlx
    close_pool(): Close specific pool
    close_all_pools(): Close all pools (used by disconnect_all())

URL Schemes:
    postgres://, postgresql://  → PostgreSQL
    mysql://                    → MySQL
    sqlite://                   → SQLite
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from datetime import timedelta
from typing import Any

from oxyde._msgpack import msgpack
from oxyde.core.types import serialize_value
from oxyde.db.registry import register_connection

try:
    from oxyde.core import close_all_pools, close_pool
    from oxyde.core import execute as _execute
    from oxyde.core import init_pool as _init_pool
    from oxyde.core import init_pool_overwrite as _init_pool_overwrite
    from oxyde.core import pool_backend as _pool_backend
except ImportError:
    # Stub for when the Rust module is not built
    async def _execute(pool_name: str, ir_bytes: bytes) -> bytes:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _init_pool(name: str, url: str, settings: dict[str, Any] | None) -> None:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _init_pool_overwrite(
        name: str, url: str, settings: dict[str, Any] | None
    ) -> None:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def close_pool(name: str) -> None:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def close_all_pools() -> None:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _pool_backend(pool_name: str) -> str:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")


def _msgpack_encoder(obj: Any) -> Any:
    """Encode non-native types for msgpack via TYPE_REGISTRY."""
    return serialize_value(obj)


def _normalize_duration(value: float | int | timedelta | None) -> float | None:
    """Convert duration to float seconds."""
    if value is None:
        return None
    if isinstance(value, timedelta):
        result = value.total_seconds()
    elif isinstance(value, (int, float)):
        result = float(value)
    else:
        raise TypeError(
            f"Duration value must be int, float, or timedelta, got {type(value).__name__}"
        )
    if result < 0:
        raise ValueError(f"Duration must be non-negative, got {result}")
    return result


def _validate_url_scheme(url: str) -> None:
    """Validate database URL scheme."""
    allowed_prefixes = ("postgres", "mysql", "sqlite")
    if not url.startswith(allowed_prefixes):
        raise ValueError(
            f"Unsupported database URL '{url}'. "
            "Supported prefixes: postgres*, mysql*, sqlite*."
        )


@dataclass
class PoolSettings:
    """Convenience container for pool configuration."""

    max_connections: int | None = None
    min_connections: int | None = None
    acquire_timeout: float | int | timedelta | None = None
    idle_timeout: float | int | timedelta | None = None
    max_lifetime: float | int | timedelta | None = None
    test_before_acquire: bool | None = None

    # Transaction cleanup settings
    transaction_timeout: float | int | timedelta | None = (
        300  # 5 minutes (max age before cleanup)
    )
    transaction_cleanup_interval: float | int | timedelta | None = (
        60  # 1 minute (cleanup interval)
    )

    # SQLite-specific PRAGMA settings (applied on connection)
    sqlite_journal_mode: str | None = (
        "WAL"  # WAL mode for better concurrent writes (10-20x faster)
    )
    sqlite_synchronous: str | None = (
        "NORMAL"  # NORMAL is a good balance between safety and speed
    )
    sqlite_cache_size: int | None = (
        10000  # Cache size in pages (~10MB with default page size)
    )
    sqlite_busy_timeout: int | None = 5000  # Timeout in milliseconds for busy database

    # TLS settings (PostgreSQL + MySQL)
    # ssl_mode controls both TLS requirement and certificate verification:
    #   PG:    "disable", "allow", "prefer", "require", "verify-ca", "verify-full"
    #   MySQL: "disabled", "preferred", "required", "verify-ca", "verify-identity"
    ssl_mode: str | None = None
    ssl_root_cert: str | None = None  # Path to CA certificate
    ssl_client_cert: str | None = None  # Path to client certificate (mTLS)
    ssl_client_key: str | None = None  # Path to client private key (mTLS)

    # PostgreSQL-specific
    pg_application_name: str | None = None  # Visible in pg_stat_activity
    pg_statement_cache_capacity: int | None = None  # Prepared statement cache size

    # MySQL-specific
    mysql_charset: str | None = None  # Character set (default: utf8mb4)
    mysql_collation: str | None = None  # Collation

    def to_payload(self) -> dict[str, Any] | None:
        payload: dict[str, Any] = {}

        if self.max_connections is not None:
            payload["max_connections"] = int(self.max_connections)
        if self.min_connections is not None:
            payload["min_connections"] = int(self.min_connections)
        if (value := _normalize_duration(self.idle_timeout)) is not None:
            payload["idle_timeout"] = value
        if (value := _normalize_duration(self.acquire_timeout)) is not None:
            payload["acquire_timeout"] = value
        if (value := _normalize_duration(self.max_lifetime)) is not None:
            payload["max_lifetime"] = value
        if self.test_before_acquire is not None:
            payload["test_before_acquire"] = bool(self.test_before_acquire)

        # Add transaction cleanup settings
        if (value := _normalize_duration(self.transaction_timeout)) is not None:
            payload["transaction_timeout"] = value
        if (
            value := _normalize_duration(self.transaction_cleanup_interval)
        ) is not None:
            payload["transaction_cleanup_interval"] = value

        # Add SQLite PRAGMA settings to payload
        if self.sqlite_journal_mode is not None:
            payload["sqlite_journal_mode"] = str(self.sqlite_journal_mode)
        if self.sqlite_synchronous is not None:
            payload["sqlite_synchronous"] = str(self.sqlite_synchronous)
        if self.sqlite_cache_size is not None:
            payload["sqlite_cache_size"] = int(self.sqlite_cache_size)
        if self.sqlite_busy_timeout is not None:
            payload["sqlite_busy_timeout"] = int(self.sqlite_busy_timeout)

        # TLS settings
        if self.ssl_mode is not None:
            payload["ssl_mode"] = str(self.ssl_mode)
        if self.ssl_root_cert is not None:
            payload["ssl_root_cert"] = str(self.ssl_root_cert)
        if self.ssl_client_cert is not None:
            payload["ssl_client_cert"] = str(self.ssl_client_cert)
        if self.ssl_client_key is not None:
            payload["ssl_client_key"] = str(self.ssl_client_key)

        # PostgreSQL-specific
        if self.pg_application_name is not None:
            payload["pg_application_name"] = str(self.pg_application_name)
        if self.pg_statement_cache_capacity is not None:
            payload["pg_statement_cache_capacity"] = int(
                self.pg_statement_cache_capacity
            )

        # MySQL-specific
        if self.mysql_charset is not None:
            payload["mysql_charset"] = str(self.mysql_charset)
        if self.mysql_collation is not None:
            payload["mysql_collation"] = str(self.mysql_collation)

        return payload or None


class AsyncDatabase:
    """Async database connection manager."""

    def __init__(
        self,
        url: str,
        *,
        name: str = "default",
        settings: PoolSettings | None = None,
        auto_register: bool = True,
        overwrite: bool = False,
    ):
        """
        Initialize database connection wrapper.

        Args:
            url: Database connection URL (e.g., "postgresql://user:pass@host/db").
            name: Identifier of the pool (used to look it up in Rust registry).
            settings: Optional pool configuration.
            auto_register: Automatically store this instance for later retrieval.
            overwrite: Replace previously registered connection with the same name.
        """
        _validate_url_scheme(url)

        self.url = url
        self.name = name
        self.settings = settings or PoolSettings()
        self.backend: str | None = None
        self._connected = False
        self._connect_lock = asyncio.Lock()
        self._overwrite = overwrite

        if auto_register:
            register_connection(self, overwrite=overwrite)

    @property
    def connected(self) -> bool:
        """Return True if the connection pool has been initialised."""
        return self._connected

    async def connect(self) -> None:
        """Establish database connection pool."""
        async with self._connect_lock:
            if self._connected:
                return

            payload = self.settings.to_payload()
            if self._overwrite:
                await _init_pool_overwrite(self.name, self.url, payload)
            else:
                await _init_pool(self.name, self.url, payload)
            self.backend = await _pool_backend(self.name)
            self._connected = True

    async def disconnect(self) -> None:
        """Close database connection pool."""
        async with self._connect_lock:
            if not self._connected:
                return

            await close_pool(self.name)
            self._connected = False

    async def ensure_connected(self) -> None:
        """Connect on demand if not connected yet."""
        if not self._connected:
            await self.connect()

    async def execute(self, ir: dict[str, Any]) -> bytes:
        """
        Execute a query using the IR format against this database.

        Args:
            ir: Query intermediate representation.

        Returns:
            MessagePack bytes containing query results.
        """
        if not self._connected:
            raise RuntimeError(
                f"Database '{self.name}' not connected. Call connect() first."
            )

        ir_bytes = msgpack.packb(ir, default=_msgpack_encoder)
        result_bytes: bytes = await _execute(self.name, ir_bytes)
        return result_bytes

    async def __aenter__(self) -> AsyncDatabase:
        """Context manager entry."""
        await self.connect()
        return self

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Context manager exit."""
        await self.disconnect()


__all__ = [
    "AsyncDatabase",
    "PoolSettings",
    "close_all_pools",
]

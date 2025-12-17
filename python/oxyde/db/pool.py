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
from datetime import datetime, timedelta
from typing import Any

try:
    from oxyde.core import close_all_pools, close_pool
    from oxyde.core import execute as _execute
    from oxyde.core import execute_select_batched as _execute_select_batched
    from oxyde.core import execute_select_batched_dedup as _execute_batched_dedup
    from oxyde.core import execute_to_pylist as _execute_to_pylist
    from oxyde.core import init_pool as _init_pool
    from oxyde.core import init_pool_overwrite as _init_pool_overwrite
except ImportError:
    # Stub for when the Rust module is not built
    async def _execute(pool_name: str, ir_bytes: bytes) -> bytes:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _execute_to_pylist(
        pool_name: str, ir_bytes: bytes
    ) -> list[dict[str, Any]]:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _execute_select_batched(
        pool_name: str, ir_bytes: bytes, batch_size: int | None = None
    ) -> list[dict[str, Any]]:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _execute_batched_dedup(
        pool_name: str, ir_bytes: bytes, batch_size: int | None = None
    ) -> dict[str, Any]:
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


def _datetime_encoder(obj):
    """Encode datetime objects for msgpack."""
    if isinstance(obj, datetime):
        return obj.isoformat()
    return obj


def _normalize_duration(value: float | int | timedelta | None) -> float | None:
    """Convert duration to float seconds."""
    if value is None:
        return None
    if isinstance(value, timedelta):
        return value.total_seconds()
    if isinstance(value, (int, float)):
        return float(value)
    raise TypeError(
        f"Duration value must be int, float, or timedelta, got {type(value).__name__}"
    )


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

    # Batch size for streaming queries (JOIN with dedup)
    # None = no batching (use fetch_all), 1000 = default batch size
    batch_size: int | None = 1000

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
        self._connected = False
        self._connect_lock = asyncio.Lock()
        self._overwrite = overwrite

        if auto_register:
            from oxyde.db.registry import register_connection

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

        import msgpack

        ir_bytes = msgpack.packb(ir, default=_datetime_encoder)
        result_bytes = await _execute(self.name, ir_bytes)
        return result_bytes

    async def execute_to_pylist(
        self, ir: dict[str, Any], *, batch_size: int | None = 500
    ) -> list[dict[str, Any]]:
        """
        Execute a SELECT query and return list[dict] directly (no msgpack).

        Uses batch streaming internally for lower memory usage:
        - Reads rows in batches of `batch_size`
        - Converts each batch to PyDict
        - Frees Rust memory after each batch

        This reduces peak memory by ~66% compared to fetch_all() approach.

        Args:
            ir: Query intermediate representation.
            batch_size: Number of rows to process per batch. Default 500.
                       Set to None to use fetch_all() (higher memory).

        Returns:
            List of dicts (one per row).
        """
        if not self._connected:
            raise RuntimeError(
                f"Database '{self.name}' not connected. Call connect() first."
            )

        import msgpack

        ir_bytes = msgpack.packb(ir, default=_datetime_encoder)

        # Use batched execution for lower memory
        if batch_size is not None:
            return await _execute_select_batched(self.name, ir_bytes, batch_size)
        else:
            return await _execute_to_pylist(self.name, ir_bytes)

    async def execute_batched_dedup(self, ir: dict[str, Any]) -> dict[str, Any]:
        """
        Execute a SELECT with JOINs and return deduplicated structure.

        Uses batch_size from PoolSettings (default 1000).

        Returns:
            {
                "main": [row_dict, ...],
                "relations": {"path": {pk: related_dict, ...}, ...}
            }

        This saves ~38% memory for JOIN queries by not duplicating related data.
        """
        if not self._connected:
            raise RuntimeError(
                f"Database '{self.name}' not connected. Call connect() first."
            )

        import msgpack

        ir_bytes = msgpack.packb(ir, default=_datetime_encoder)
        batch_size = self.settings.batch_size
        return await _execute_batched_dedup(self.name, ir_bytes, batch_size)

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

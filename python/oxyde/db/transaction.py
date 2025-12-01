"""Transaction management with Django-style atomic() context manager.

This module provides transaction support with automatic commit/rollback
and nested transaction handling via savepoints.

Usage:
    from oxyde.db import transaction

    # Basic transaction
    async with transaction.atomic():
        await User.objects.create(name="Alice")
        await Order.objects.create(user_id=1)
        # Commits on exit, rolls back on exception

    # Explicit rollback
    async with transaction.atomic() as tx:
        await User.objects.create(name="Alice")
        tx.set_rollback()  # Force rollback even without exception

    # Nested transactions (savepoints)
    async with transaction.atomic():
        await User.objects.create(name="Alice")
        try:
            async with transaction.atomic():
                await User.objects.create(name="Bob")
                raise ValueError("oops")
        except ValueError:
            pass  # Bob rolled back to savepoint, Alice still pending
        # Alice commits

Classes:
    AsyncTransaction:
        Low-level transaction wrapper. Holds tx_id from Rust.
        execute() sends queries through the transaction.

    AtomicTransactionContext:
        High-level context manager for atomic(). Manages nesting
        via savepoints. Tracks depth per connection alias.

    TransactionTimeoutError:
        Raised when transaction exceeds configured timeout.

Functions:
    atomic(using="default", timeout=None):
        Create atomic context manager. Django-compatible API.

    get_active_transaction(using="default"):
        Get current transaction for connection (or None).
        Used by QueryManager to route queries through active tx.

ContextVar Storage:
    _ACTIVE_TRANSACTIONS stores {alias: {transaction, depth, force_rollback}}
    per async context. This allows concurrent requests to have separate
    transaction state.

Savepoint Handling:
    Nested atomic() calls create savepoints (sp_1, sp_2, ...).
    Inner exception rolls back to savepoint, not entire transaction.
    This matches Django/PostgreSQL behavior.

Rust Integration:
    _begin_transaction(pool_name) → tx_id
    _execute_in_transaction(pool_name, tx_id, ir_bytes) → bytes
    _commit_transaction(tx_id)
    _rollback_transaction(tx_id)
    _create_savepoint(tx_id, name)
    _rollback_to_savepoint(tx_id, name)
    _release_savepoint(tx_id, name)
"""

from __future__ import annotations

import asyncio
from contextvars import ContextVar
from datetime import timedelta
from typing import TYPE_CHECKING, Any

from oxyde.db.pool import _datetime_encoder, _normalize_duration

if TYPE_CHECKING:
    from oxyde.db.pool import AsyncDatabase

try:
    from oxyde.core import begin_transaction as _begin_transaction
    from oxyde.core import commit_transaction as _commit_transaction
    from oxyde.core import create_savepoint as _create_savepoint
    from oxyde.core import execute_in_transaction as _execute_in_transaction
    from oxyde.core import release_savepoint as _release_savepoint
    from oxyde.core import rollback_to_savepoint as _rollback_to_savepoint
    from oxyde.core import rollback_transaction as _rollback_transaction
except ImportError:
    # Stub for when the Rust module is not built
    async def _execute_in_transaction(
        pool_name: str, tx_id: int, ir_bytes: bytes
    ) -> bytes:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _begin_transaction(pool_name: str) -> int:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _commit_transaction(tx_id: int) -> None:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _rollback_transaction(tx_id: int) -> None:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _create_savepoint(tx_id: int, savepoint_name: str) -> None:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _rollback_to_savepoint(tx_id: int, savepoint_name: str) -> None:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")

    async def _release_savepoint(tx_id: int, savepoint_name: str) -> None:
        raise RuntimeError("Rust core module not available. Please install oxyde-core.")


# Active transactions per connection (ContextVar for async safety)
_ACTIVE_TRANSACTIONS: ContextVar[dict[str, dict[str, Any]]] = ContextVar(
    "oxyde_active_transactions",
    default={},
)


class TransactionTimeoutError(TimeoutError):
    """Raised when a transaction exceeds the configured timeout."""


class AsyncTransaction:
    """Wrapper for executing queries within a database transaction."""

    def __init__(self, database: AsyncDatabase, timeout: float | None = None):
        self._database = database
        self._tx_id: int | None = None
        self._timeout = timeout
        self._deadline: float | None = None
        self._timed_out = False

    @property
    def id(self) -> int:
        if self._tx_id is None:
            raise RuntimeError("Transaction not started")
        return self._tx_id

    def _remaining_timeout(self) -> float | None:
        if self._deadline is None:
            return None
        loop = asyncio.get_running_loop()
        return self._deadline - loop.time()

    async def execute(self, ir: dict[str, Any]) -> bytes:
        if self._tx_id is None:
            raise RuntimeError("Transaction not started")
        if self._timed_out:
            raise TransactionTimeoutError(
                "Transaction can no longer be used because its timeout elapsed"
            )

        import msgpack

        ir_bytes = msgpack.packb(ir, default=_datetime_encoder)
        coro = _execute_in_transaction(self._database.name, self._tx_id, ir_bytes)

        remaining = self._remaining_timeout()
        if remaining is not None:
            if remaining <= 0:
                self._timed_out = True
                msg = f"Transaction {self._tx_id} exceeded timeout"
                raise TransactionTimeoutError(msg)
            try:
                return await asyncio.wait_for(coro, remaining)
            except asyncio.TimeoutError as exc:
                self._timed_out = True
                msg = f"Transaction {self._tx_id} exceeded timeout"
                raise TransactionTimeoutError(msg) from exc

        return await coro

    async def __aenter__(self) -> AsyncTransaction:
        await self._database.ensure_connected()
        self._tx_id = await _begin_transaction(self._database.name)
        if self._timeout is not None:
            loop = asyncio.get_running_loop()
            self._deadline = loop.time() + self._timeout
        return self

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        tx_id = self._tx_id
        self._tx_id = None
        self._deadline = None

        if tx_id is None:
            return

        try:
            if self._timed_out or exc_type is not None:
                await _rollback_transaction(tx_id)
            else:
                await _commit_transaction(tx_id)
        finally:
            self._timed_out = False


class AtomicTransactionContext:
    """Context manager that reuses or creates transactions per connection alias."""

    def __init__(
        self,
        using: str = "default",
        timeout: float | None = None,
        database: AsyncDatabase | None = None,
    ):
        self.using = using
        self.timeout = timeout
        self._database = database
        self._transaction: AsyncTransaction | None = None
        self._savepoint_name: str | None = None

    async def __aenter__(self) -> AtomicTransactionContext:
        state = dict(_ACTIVE_TRANSACTIONS.get())
        entry = state.get(self.using)
        if entry:
            # Nested transaction - create savepoint
            depth = entry["depth"]
            entry["depth"] += 1
            state[self.using] = entry
            _ACTIVE_TRANSACTIONS.set(state)
            self._transaction = entry["transaction"]

            # Create savepoint for nested transaction
            self._savepoint_name = f"sp_{depth + 1}"
            await _create_savepoint(self._transaction.id, self._savepoint_name)

            return self

        # Use provided database instance or get from registry
        from oxyde.db.registry import get_connection

        database = (
            self._database
            if self._database is not None
            else await get_connection(self.using)
        )
        tx = AsyncTransaction(database, timeout=self.timeout)
        await tx.__aenter__()
        state[self.using] = {
            "transaction": tx,
            "depth": 1,
            "force_rollback": False,
        }
        _ACTIVE_TRANSACTIONS.set(state)
        self._transaction = tx
        return self

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        state = dict(_ACTIVE_TRANSACTIONS.get())
        entry = state.get(self.using)
        if entry is None:
            raise RuntimeError("transaction.atomic context mismatch")

        entry["depth"] -= 1
        force_rollback = entry.get("force_rollback", False)

        if entry["depth"] == 0:
            # Top-level transaction - commit or rollback entire transaction
            state.pop(self.using, None)
            _ACTIVE_TRANSACTIONS.set(state)
            tx = entry["transaction"]
            if exc_type is None and force_rollback:
                exc_val = RuntimeError("transaction marked for rollback")
                exc_type = RuntimeError
            await tx.__aexit__(exc_type, exc_val, exc_tb)
        else:
            # Nested transaction - handle savepoint
            if self._savepoint_name:
                if exc_type:
                    # Rollback to savepoint - this is the end of this nested transaction
                    # Don't mark outer transaction for rollback, savepoint handles it
                    await _rollback_to_savepoint(
                        entry["transaction"].id, self._savepoint_name
                    )
                else:
                    # Release savepoint - nested transaction succeeded
                    await _release_savepoint(
                        entry["transaction"].id, self._savepoint_name
                    )
                # Don't modify force_rollback - savepoint already handled the rollback
            else:
                # No savepoint (shouldn't happen with correct nesting)
                if exc_type:
                    entry["force_rollback"] = True

            state[self.using] = entry
            _ACTIVE_TRANSACTIONS.set(state)

    def set_rollback(self, rollback: bool = True) -> None:
        state = dict(_ACTIVE_TRANSACTIONS.get())
        entry = state.get(self.using)
        if entry is None:
            raise RuntimeError("No active transaction to mark for rollback")
        entry["force_rollback"] = bool(rollback)
        state[self.using] = entry
        _ACTIVE_TRANSACTIONS.set(state)

    @property
    def transaction(self) -> AsyncTransaction:
        if self._transaction is None:
            raise RuntimeError("transaction.atomic context not entered")
        return self._transaction

    async def execute(self, ir: dict[str, Any]) -> bytes:
        return await self.transaction.execute(ir)

    def __getattr__(self, item: str) -> Any:
        return getattr(self.transaction, item)


def atomic(
    *,
    using: str = "default",
    timeout: float | int | timedelta | None = None,
) -> AtomicTransactionContext:
    """Create an atomic transaction context."""
    normalized = _normalize_duration(timeout)
    return AtomicTransactionContext(using=using, timeout=normalized)


def get_active_transaction(using: str = "default") -> AsyncTransaction | None:
    """Get the currently active transaction for a connection alias."""
    entry = _ACTIVE_TRANSACTIONS.get().get(using)
    if entry:
        return entry["transaction"]
    return None


__all__ = [
    "AsyncTransaction",
    "TransactionTimeoutError",
    "AtomicTransactionContext",
    "atomic",
    "get_active_transaction",
]

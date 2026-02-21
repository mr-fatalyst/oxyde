"""Tests for transaction management: savepoints, rollback, timeout."""

from __future__ import annotations

from typing import Any

import msgpack
import pytest

from oxyde import Field, Model
from oxyde.db import atomic
from oxyde.db.transaction import (
    _ACTIVE_TRANSACTIONS,
    AsyncTransaction,
    get_active_transaction,
)
from oxyde.models.registry import clear_registry


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


@pytest.fixture
def reset_transactions():
    """Reset transaction state before each test."""
    _ACTIVE_TRANSACTIONS.set({})
    yield
    _ACTIVE_TRANSACTIONS.set({})


class TestModel(Model):
    """Test model for transaction tests."""

    id: int | None = Field(default=None, db_pk=True)
    name: str
    value: int = 0

    class Meta:
        is_table = True


class DummyDatabase:
    """Dummy database for testing."""

    def __init__(self, name: str = "default"):
        self.name = name
        self._connected = True

    async def ensure_connected(self):
        pass


class MockTransactionModule:
    """Mock transaction module functions."""

    def __init__(self):
        self.calls: list[tuple[str, Any]] = []
        self.tx_counter = 0
        self.savepoints: dict[int, list[str]] = {}

    async def begin_transaction(self, pool_name: str) -> int:
        self.tx_counter += 1
        self.calls.append(("begin", pool_name))
        self.savepoints[self.tx_counter] = []
        return self.tx_counter

    async def commit_transaction(self, tx_id: int) -> None:
        self.calls.append(("commit", tx_id))

    async def rollback_transaction(self, tx_id: int) -> None:
        self.calls.append(("rollback", tx_id))

    async def create_savepoint(self, tx_id: int, name: str) -> None:
        self.calls.append(("savepoint", (tx_id, name)))
        self.savepoints[tx_id].append(name)

    async def rollback_to_savepoint(self, tx_id: int, name: str) -> None:
        self.calls.append(("rollback_savepoint", (tx_id, name)))

    async def release_savepoint(self, tx_id: int, name: str) -> None:
        self.calls.append(("release_savepoint", (tx_id, name)))

    async def execute_in_transaction(
        self, pool_name: str, tx_id: int, ir_bytes: bytes
    ) -> bytes:
        self.calls.append(("execute", (pool_name, tx_id)))
        return msgpack.packb([])


@pytest.fixture
def mock_tx(monkeypatch, reset_transactions):
    """Fixture that mocks transaction functions."""
    import sys

    # Get the actual transaction module from sys.modules
    # (not oxyde.db.transaction alias which is the atomic function)
    transaction_module = sys.modules["oxyde.db.transaction"]

    mock = MockTransactionModule()

    # Patch the private functions directly on the module object
    monkeypatch.setattr(
        transaction_module, "_begin_transaction", mock.begin_transaction
    )
    monkeypatch.setattr(
        transaction_module, "_commit_transaction", mock.commit_transaction
    )
    monkeypatch.setattr(
        transaction_module, "_rollback_transaction", mock.rollback_transaction
    )
    monkeypatch.setattr(transaction_module, "_create_savepoint", mock.create_savepoint)
    monkeypatch.setattr(
        transaction_module, "_rollback_to_savepoint", mock.rollback_to_savepoint
    )
    monkeypatch.setattr(
        transaction_module, "_release_savepoint", mock.release_savepoint
    )
    monkeypatch.setattr(
        transaction_module, "_execute_in_transaction", mock.execute_in_transaction
    )

    return mock


@pytest.fixture
def mock_get_connection(monkeypatch):
    """Mock get_connection to return dummy database."""

    async def get_conn(name: str = "default", ensure_connected: bool = True):
        return DummyDatabase(name)

    monkeypatch.setattr("oxyde.db.registry.get_connection", get_conn)
    monkeypatch.setattr("oxyde.db.transaction.get_connection", get_conn)


class TestAsyncTransaction:
    """Test AsyncTransaction class."""

    @pytest.mark.asyncio
    async def test_transaction_context_manager(self, mock_tx, mock_get_connection):
        """Test transaction as context manager."""
        db = DummyDatabase()
        async with AsyncTransaction(db) as tx:
            assert tx._tx_id is not None

        # Should have begun and committed
        assert ("begin", "default") in mock_tx.calls
        assert ("commit", 1) in mock_tx.calls

    @pytest.mark.asyncio
    async def test_transaction_rollback_on_exception(
        self, mock_tx, mock_get_connection
    ):
        """Test transaction rollback on exception."""
        db = DummyDatabase()

        with pytest.raises(ValueError):
            async with AsyncTransaction(db) as tx:
                raise ValueError("test error")

        # Should have begun and rolled back
        assert ("begin", "default") in mock_tx.calls
        assert ("rollback", 1) in mock_tx.calls

    @pytest.mark.asyncio
    async def test_transaction_execute(self, mock_tx, mock_get_connection):
        """Test executing query in transaction."""
        db = DummyDatabase()

        async with AsyncTransaction(db) as tx:
            await tx.execute({"op": "select", "table": "test"})

        assert ("execute", ("default", 1)) in mock_tx.calls


class TestAtomicContext:
    """Test atomic() context manager."""

    @pytest.mark.asyncio
    async def test_atomic_basic(self, mock_tx, mock_get_connection):
        """Test basic atomic() usage."""
        async with atomic() as ctx:
            assert ctx.transaction is not None

        assert ("begin", "default") in mock_tx.calls
        assert ("commit", 1) in mock_tx.calls

    @pytest.mark.asyncio
    async def test_atomic_rollback_on_exception(self, mock_tx, mock_get_connection):
        """Test atomic() rollback on exception."""
        with pytest.raises(RuntimeError):
            async with atomic():
                raise RuntimeError("error")

        assert ("rollback", 1) in mock_tx.calls

    @pytest.mark.asyncio
    async def test_atomic_set_rollback(self, mock_tx, mock_get_connection):
        """Test atomic() with set_rollback()."""
        async with atomic() as ctx:
            ctx.set_rollback(True)

        # Should have rolled back due to set_rollback
        assert ("rollback", 1) in mock_tx.calls

    @pytest.mark.asyncio
    async def test_atomic_with_using(self, mock_tx, mock_get_connection):
        """Test atomic() with using parameter."""
        async with atomic(using="other") as ctx:
            pass

        assert ("begin", "other") in mock_tx.calls


class TestNestedTransactions:
    """Test nested transactions (savepoints)."""

    @pytest.mark.asyncio
    async def test_nested_creates_savepoint(self, mock_tx, mock_get_connection):
        """Test nested atomic() creates savepoint."""
        async with atomic() as outer:
            async with atomic() as inner:
                pass

        # Should have created savepoint
        savepoint_calls = [c for c in mock_tx.calls if c[0] == "savepoint"]
        assert len(savepoint_calls) == 1

    @pytest.mark.asyncio
    async def test_nested_releases_savepoint_on_success(
        self, mock_tx, mock_get_connection
    ):
        """Test nested transaction releases savepoint on success."""
        async with atomic():
            async with atomic():
                pass

        release_calls = [c for c in mock_tx.calls if c[0] == "release_savepoint"]
        assert len(release_calls) == 1

    @pytest.mark.asyncio
    async def test_nested_rollbacks_to_savepoint_on_error(
        self, mock_tx, mock_get_connection
    ):
        """Test nested transaction rollbacks to savepoint on error."""
        async with atomic():
            try:
                async with atomic():
                    raise ValueError("inner error")
            except ValueError:
                pass

        rollback_sp_calls = [c for c in mock_tx.calls if c[0] == "rollback_savepoint"]
        assert len(rollback_sp_calls) == 1

    @pytest.mark.asyncio
    async def test_deeply_nested_transactions(self, mock_tx, mock_get_connection):
        """Test deeply nested transactions."""
        async with atomic():
            async with atomic():
                async with atomic():
                    pass

        # Should have created 2 savepoints (depth 2 and 3)
        savepoint_calls = [c for c in mock_tx.calls if c[0] == "savepoint"]
        assert len(savepoint_calls) == 2

    @pytest.mark.asyncio
    async def test_nested_exception_doesnt_rollback_outer(
        self, mock_tx, mock_get_connection
    ):
        """Test nested exception with savepoint doesn't force outer rollback."""
        async with atomic():
            try:
                async with atomic():
                    raise ValueError("inner")
            except ValueError:
                pass
            # Continue in outer transaction

        # Outer should commit, not rollback
        assert ("commit", 1) in mock_tx.calls


class TestTransactionTimeout:
    """Test transaction timeout handling."""

    @pytest.mark.asyncio
    async def test_transaction_with_timeout(self, mock_tx, mock_get_connection):
        """Test transaction with timeout parameter."""
        async with atomic(timeout=5.0) as ctx:
            assert ctx.timeout == 5.0

    @pytest.mark.asyncio
    async def test_timeout_as_timedelta(self, mock_tx, mock_get_connection):
        """Test timeout as timedelta."""
        from datetime import timedelta

        async with atomic(timeout=timedelta(seconds=10)) as ctx:
            # Timeout should be normalized to float
            pass


class TestGetActiveTransaction:
    """Test get_active_transaction() function."""

    @pytest.mark.asyncio
    async def test_get_active_transaction_returns_transaction(
        self, mock_tx, mock_get_connection
    ):
        """Test get_active_transaction() returns active transaction."""
        async with atomic() as ctx:
            active = get_active_transaction("default")
            assert active is ctx.transaction

    @pytest.mark.asyncio
    async def test_get_active_transaction_returns_none_outside(
        self, mock_tx, mock_get_connection, reset_transactions
    ):
        """Test get_active_transaction() returns None outside transaction."""
        active = get_active_transaction("default")
        assert active is None

    @pytest.mark.asyncio
    async def test_get_active_transaction_different_alias(
        self, mock_tx, mock_get_connection
    ):
        """Test get_active_transaction() with different alias."""
        async with atomic(using="primary") as ctx:
            # "default" should have no active transaction
            assert get_active_transaction("default") is None
            # "primary" should have active transaction
            assert get_active_transaction("primary") is ctx.transaction


class TestTransactionIsolation:
    """Test transaction isolation between aliases."""

    @pytest.mark.asyncio
    async def test_separate_transactions_per_alias(self, mock_tx, mock_get_connection):
        """Test that different aliases have separate transactions."""
        async with atomic(using="db1") as ctx1:
            async with atomic(using="db2") as ctx2:
                assert ctx1.transaction is not ctx2.transaction
                assert get_active_transaction("db1") is ctx1.transaction
                assert get_active_transaction("db2") is ctx2.transaction


class TestTransactionExecute:
    """Test query execution within transactions."""

    @pytest.mark.asyncio
    async def test_execute_uses_transaction(self, mock_tx, mock_get_connection):
        """Test that execute uses the transaction."""
        async with atomic() as ctx:
            await ctx.execute({"op": "select", "table": "test"})

        execute_calls = [c for c in mock_tx.calls if c[0] == "execute"]
        assert len(execute_calls) == 1

    @pytest.mark.asyncio
    async def test_execute_after_exit_raises(self, mock_tx, mock_get_connection):
        """Test executing after transaction exit raises error."""
        db = DummyDatabase()
        tx = AsyncTransaction(db)

        async with tx:
            pass

        with pytest.raises(RuntimeError):
            await tx.execute({"op": "select"})


class TestTransactionCleanup:
    """Test transaction cleanup behavior."""

    @pytest.mark.asyncio
    async def test_cleanup_on_normal_exit(
        self, mock_tx, mock_get_connection, reset_transactions
    ):
        """Test cleanup after normal exit."""
        async with atomic():
            pass

        assert get_active_transaction("default") is None

    @pytest.mark.asyncio
    async def test_cleanup_on_exception(
        self, mock_tx, mock_get_connection, reset_transactions
    ):
        """Test cleanup after exception."""
        with pytest.raises(ValueError):
            async with atomic():
                raise ValueError()

        assert get_active_transaction("default") is None

    @pytest.mark.asyncio
    async def test_nested_cleanup(
        self, mock_tx, mock_get_connection, reset_transactions
    ):
        """Test nested transaction cleanup."""
        async with atomic():
            async with atomic():
                pass
            # Inner cleaned up, outer still active
            assert get_active_transaction("default") is not None

        # Both cleaned up
        assert get_active_transaction("default") is None

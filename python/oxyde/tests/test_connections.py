import asyncio
import importlib

import pytest

from oxyde.db.pool import AsyncDatabase, PoolSettings, _validate_url_scheme
from oxyde.db.registry import disconnect_all, get_connection, register_connection
from oxyde.db.transaction import TransactionTimeoutError, atomic

# Import modules for monkeypatching (avoid name collisions with __init__.py exports)
_pool_module = importlib.import_module("oxyde.db.pool")
_tx_module = importlib.import_module("oxyde.db.transaction")
_reg_module = importlib.import_module("oxyde.db.registry")


class StubCore:
    def __init__(self) -> None:
        self.init_calls: list[tuple[str, str, dict | None]] = []
        self.close_calls: list[str] = []
        self.close_all_called = False
        self.begin_calls: list[str] = []
        self.commit_calls: list[int] = []
        self.rollback_calls: list[int] = []
        self.execute_in_tx_calls: list[tuple[int, bytes]] = []
        self.tx_counter = 1

    async def init_pool(self, name: str, url: str, settings: dict | None) -> None:
        self.init_calls.append((name, url, settings))

    async def close_pool(self, name: str) -> None:
        self.close_calls.append(name)

    async def close_all_pools(self) -> None:
        self.close_all_called = True

    async def execute(self, pool_name: str, ir_bytes: bytes) -> bytes:
        raise AssertionError("execute should not be called in this test")

    async def begin_transaction(self, pool_name: str) -> int:
        self.begin_calls.append(pool_name)
        tx_id = self.tx_counter
        self.tx_counter += 1
        return tx_id

    async def commit_transaction(self, tx_id: int) -> None:
        self.commit_calls.append(tx_id)

    async def rollback_transaction(self, tx_id: int) -> None:
        self.rollback_calls.append(tx_id)

    async def execute_in_transaction(
        self, pool_name: str, tx_id: int, ir_bytes: bytes
    ) -> bytes:
        self.execute_in_tx_calls.append((tx_id, ir_bytes))
        return b"{}"


@pytest.mark.asyncio
async def test_register_and_retrieve_connection(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stub = StubCore()
    monkeypatch.setattr(_reg_module, "_CONNECTIONS", {})
    monkeypatch.setattr(_pool_module, "_init_pool", stub.init_pool)
    monkeypatch.setattr(_pool_module, "_init_pool_overwrite", stub.init_pool)
    monkeypatch.setattr(_pool_module, "close_pool", stub.close_pool)
    monkeypatch.setattr(_pool_module, "close_all_pools", stub.close_all_pools)
    monkeypatch.setattr(_pool_module, "_execute", stub.execute)
    monkeypatch.setattr(
        _tx_module, "_execute_in_transaction", stub.execute_in_transaction
    )
    monkeypatch.setattr(_tx_module, "_begin_transaction", stub.begin_transaction)
    monkeypatch.setattr(_tx_module, "_commit_transaction", stub.commit_transaction)
    monkeypatch.setattr(_tx_module, "_rollback_transaction", stub.rollback_transaction)

    db = AsyncDatabase(
        "sqlite::memory:",
        name="analytics",
        settings=PoolSettings(max_connections=1, min_connections=1),
        auto_register=False,
    )
    register_connection(db, overwrite=True)

    assert db.connected is False
    await db.connect()
    assert db.connected is True

    assert stub.init_calls == [
        (
            "analytics",
            "sqlite::memory:",
            {
                "max_connections": 1,
                "min_connections": 1,
                "transaction_timeout": 300.0,
                "transaction_cleanup_interval": 60.0,
                "sqlite_journal_mode": "WAL",
                "sqlite_synchronous": "NORMAL",
                "sqlite_cache_size": 10000,
                "sqlite_busy_timeout": 5000,
            },
        )
    ]

    fetched = await get_connection("analytics", ensure_connected=False)
    assert fetched is db

    await disconnect_all()
    # disconnect_all() only calls close_all_pools(), not individual close_pool()
    assert stub.close_all_called is True


@pytest.mark.asyncio
async def test_transaction_context(monkeypatch: pytest.MonkeyPatch) -> None:
    stub = StubCore()
    monkeypatch.setattr(_pool_module, "_init_pool", stub.init_pool)
    monkeypatch.setattr(_pool_module, "_init_pool_overwrite", stub.init_pool)
    monkeypatch.setattr(_pool_module, "close_pool", stub.close_pool)
    monkeypatch.setattr(_pool_module, "close_all_pools", stub.close_all_pools)
    monkeypatch.setattr(_pool_module, "_execute", stub.execute)
    monkeypatch.setattr(
        _tx_module, "_execute_in_transaction", stub.execute_in_transaction
    )
    monkeypatch.setattr(_tx_module, "_begin_transaction", stub.begin_transaction)
    monkeypatch.setattr(_tx_module, "_commit_transaction", stub.commit_transaction)
    monkeypatch.setattr(_tx_module, "_rollback_transaction", stub.rollback_transaction)

    # Clear connections after setting up monkeypatch
    monkeypatch.setattr(_reg_module, "_CONNECTIONS", {})

    db = AsyncDatabase(
        "sqlite::memory:",
        name="default",
        settings=PoolSettings(max_connections=1, min_connections=1),
        auto_register=True,  # Register so atomic() can find it
    )

    async with atomic(using=db.name) as tx:
        await tx.execute({"op": "select", "table": "widgets"})

    assert stub.begin_calls == ["default"]
    assert stub.commit_calls == [1]
    assert stub.rollback_calls == []
    assert len(stub.execute_in_tx_calls) == 1

    # Trigger rollback branch
    try:
        async with atomic(using=db.name) as tx:
            await tx.execute({"op": "select", "table": "widgets"})
            raise RuntimeError("boom")
    except RuntimeError:
        pass

    assert stub.begin_calls == ["default", "default"]
    assert stub.commit_calls == [1]
    assert stub.rollback_calls == [2]
    assert len(stub.execute_in_tx_calls) == 2


@pytest.mark.asyncio
async def test_transaction_timeout_rolls_back(monkeypatch: pytest.MonkeyPatch) -> None:
    stub = StubCore()

    async def slow_execute_in_transaction(
        pool_name: str, tx_id: int, ir_bytes: bytes
    ) -> bytes:
        await asyncio.sleep(0.05)
        stub.execute_in_tx_calls.append((tx_id, ir_bytes))
        return b"{}"

    monkeypatch.setattr(_pool_module, "_init_pool", stub.init_pool)
    monkeypatch.setattr(_pool_module, "_init_pool_overwrite", stub.init_pool)
    monkeypatch.setattr(_pool_module, "close_pool", stub.close_pool)
    monkeypatch.setattr(_pool_module, "close_all_pools", stub.close_all_pools)
    monkeypatch.setattr(_pool_module, "_execute", stub.execute)
    monkeypatch.setattr(
        _tx_module, "_execute_in_transaction", slow_execute_in_transaction
    )
    monkeypatch.setattr(_tx_module, "_begin_transaction", stub.begin_transaction)
    monkeypatch.setattr(_tx_module, "_commit_transaction", stub.commit_transaction)
    monkeypatch.setattr(_tx_module, "_rollback_transaction", stub.rollback_transaction)

    # Clear connections after setting up monkeypatch
    monkeypatch.setattr(_reg_module, "_CONNECTIONS", {})

    db = AsyncDatabase(
        "sqlite::memory:",
        name="default",
        settings=PoolSettings(max_connections=1, min_connections=1),
        auto_register=True,  # Register so atomic() can find it
    )

    with pytest.raises(TransactionTimeoutError):
        async with atomic(using=db.name, timeout=0.01) as tx:
            await tx.execute({"op": "select", "table": "widgets"})

    assert stub.begin_calls == ["default"]
    assert stub.commit_calls == []
    assert stub.rollback_calls == [1]


def test_validate_url_scheme() -> None:
    _validate_url_scheme("postgresql://user:pass@localhost/db")
    _validate_url_scheme("mysql://user:pass@localhost/db")
    _validate_url_scheme("sqlite::memory:")

    with pytest.raises(ValueError):
        _validate_url_scheme("mongodb://localhost")

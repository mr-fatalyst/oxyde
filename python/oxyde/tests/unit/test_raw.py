"""Tests for execute_raw function."""

from __future__ import annotations

from typing import Any

import msgpack
import pytest

from oxyde.db.registry import _CONNECTIONS
from oxyde.db.transaction import _ACTIVE_TRANSACTIONS
from oxyde.exceptions import ManagerError
from oxyde.queries.raw import execute_raw


class StubExecuteClient:
    """Stub client that records calls and returns columnar data."""

    def __init__(self, columns: list[str], rows: list[list]):
        self.columns = columns
        self.rows = rows
        self.calls: list[dict[str, Any]] = []
        self.name = "stub"

    async def execute(self, ir: dict[str, Any]) -> bytes:
        self.calls.append(ir)
        return msgpack.packb((self.columns, self.rows))


class StubDatabase:
    """Stub database for registry."""

    def __init__(self, name: str = "default"):
        self.name = name
        self._connected = True
        self.calls: list[dict] = []

    @property
    def connected(self):
        return self._connected

    async def connect(self):
        self._connected = True

    async def execute(self, ir: dict[str, Any]) -> bytes:
        self.calls.append(ir)
        return msgpack.packb((["id", "name"], [[1, "Alice"], [2, "Bob"]]))


@pytest.fixture
def reset_state():
    """Reset registry and transactions."""
    _CONNECTIONS.clear()
    _ACTIVE_TRANSACTIONS.set({})
    yield
    _CONNECTIONS.clear()
    _ACTIVE_TRANSACTIONS.set({})


class TestExecuteRawIR:
    """Test IR generation."""

    @pytest.mark.asyncio
    async def test_builds_correct_ir(self, reset_state):
        """execute_raw builds correct IR structure."""
        stub = StubExecuteClient(["id"], [[1]])

        await execute_raw("SELECT 1", client=stub)

        assert len(stub.calls) == 1
        ir = stub.calls[0]
        assert ir["op"] == "raw"
        assert ir["proto"] == 1
        assert ir["sql"] == "SELECT 1"
        assert ir["params"] == []
        assert ir["table"] == ""

    @pytest.mark.asyncio
    async def test_passes_params(self, reset_state):
        """execute_raw passes params correctly."""
        stub = StubExecuteClient(["id"], [[1]])

        await execute_raw(
            "SELECT * FROM users WHERE age > $1 AND status = $2",
            [18, "active"],
            client=stub,
        )

        ir = stub.calls[0]
        assert ir["params"] == [18, "active"]

    @pytest.mark.asyncio
    async def test_none_params_becomes_empty_list(self, reset_state):
        """None params converted to empty list."""
        stub = StubExecuteClient(["x"], [[1]])

        await execute_raw("SELECT 1", None, client=stub)

        assert stub.calls[0]["params"] == []


class TestExecuteRawResult:
    """Test result conversion."""

    @pytest.mark.asyncio
    async def test_converts_columnar_to_dicts(self, reset_state):
        """Columnar (columns, rows) converted to list[dict]."""
        stub = StubExecuteClient(
            ["id", "name", "age"],
            [[1, "Alice", 30], [2, "Bob", 25]],
        )

        result = await execute_raw("SELECT ...", client=stub)

        assert result == [
            {"id": 1, "name": "Alice", "age": 30},
            {"id": 2, "name": "Bob", "age": 25},
        ]

    @pytest.mark.asyncio
    async def test_empty_result(self, reset_state):
        """Empty result returns empty list."""
        stub = StubExecuteClient(["id"], [])

        result = await execute_raw("SELECT ...", client=stub)

        assert result == []

    @pytest.mark.asyncio
    async def test_single_row(self, reset_state):
        """Single row result."""
        stub = StubExecuteClient(["count"], [[42]])

        result = await execute_raw("SELECT COUNT(*)", client=stub)

        assert result == [{"count": 42}]


class TestExecuteRawConnection:
    """Test connection resolution."""

    @pytest.mark.asyncio
    async def test_uses_explicit_client(self, reset_state):
        """Explicit client is used."""
        stub = StubExecuteClient(["x"], [[1]])

        await execute_raw("SELECT 1", client=stub)

        assert len(stub.calls) == 1

    @pytest.mark.asyncio
    async def test_uses_registry_default(self, reset_state):
        """Uses 'default' from registry when no client/using."""
        db = StubDatabase("default")
        _CONNECTIONS["default"] = db

        await execute_raw("SELECT 1")

        assert len(db.calls) == 1

    @pytest.mark.asyncio
    async def test_uses_named_connection(self, reset_state):
        """using='analytics' uses that connection."""
        db = StubDatabase("analytics")
        _CONNECTIONS["analytics"] = db

        await execute_raw("SELECT 1", using="analytics")

        assert len(db.calls) == 1

    @pytest.mark.asyncio
    async def test_error_when_both_client_and_using(self, reset_state):
        """Error when both client and using provided."""
        stub = StubExecuteClient(["x"], [[1]])

        with pytest.raises(ManagerError):
            await execute_raw("SELECT 1", client=stub, using="default")

    @pytest.mark.asyncio
    async def test_error_when_connection_not_found(self, reset_state):
        """Error when connection not in registry."""
        with pytest.raises(KeyError, match="not registered"):
            await execute_raw("SELECT 1", using="nonexistent")


class TestExecuteRawTransaction:
    """Test transaction integration."""

    @pytest.mark.asyncio
    async def test_uses_active_transaction(self, reset_state):
        """Uses active transaction when inside atomic()."""
        tx_stub = StubExecuteClient(["x"], [[1]])
        tx_stub._database = StubDatabase("default")

        # Simulate active transaction
        _ACTIVE_TRANSACTIONS.set({
            "default": {
                "transaction": tx_stub,
                "depth": 1,
                "force_rollback": False,
            }
        })

        # Register db but should use tx instead
        db = StubDatabase("default")
        _CONNECTIONS["default"] = db

        await execute_raw("SELECT 1")

        # Transaction was used, not db directly
        assert len(tx_stub.calls) == 1
        assert len(db.calls) == 0

    @pytest.mark.asyncio
    async def test_uses_db_when_no_active_transaction(self, reset_state):
        """Uses db when no active transaction."""
        db = StubDatabase("default")
        _CONNECTIONS["default"] = db

        # No active transaction
        _ACTIVE_TRANSACTIONS.set({})

        await execute_raw("SELECT 1")

        assert len(db.calls) == 1

    @pytest.mark.asyncio
    async def test_respects_using_for_transaction_lookup(self, reset_state):
        """Transaction lookup uses 'using' alias."""
        tx_default = StubExecuteClient(["x"], [[1]])
        tx_default._database = StubDatabase("default")

        tx_analytics = StubExecuteClient(["y"], [[2]])
        tx_analytics._database = StubDatabase("analytics")

        _ACTIVE_TRANSACTIONS.set({
            "default": {
                "transaction": tx_default,
                "depth": 1,
                "force_rollback": False,
            },
            "analytics": {
                "transaction": tx_analytics,
                "depth": 1,
                "force_rollback": False,
            },
        })

        db_default = StubDatabase("default")
        db_analytics = StubDatabase("analytics")
        _CONNECTIONS["default"] = db_default
        _CONNECTIONS["analytics"] = db_analytics

        # Should use analytics transaction
        await execute_raw("SELECT 1", using="analytics")

        assert len(tx_analytics.calls) == 1
        assert len(tx_default.calls) == 0


class TestExecuteRawEdgeCases:
    """Test edge cases."""

    @pytest.mark.asyncio
    async def test_multiline_sql(self, reset_state):
        """Multiline SQL preserved."""
        stub = StubExecuteClient(["id"], [[1]])

        sql = """
            SELECT id, name
            FROM users
            WHERE age > $1
            ORDER BY name
        """
        await execute_raw(sql, [18], client=stub)

        assert stub.calls[0]["sql"] == sql

    @pytest.mark.asyncio
    async def test_complex_params(self, reset_state):
        """Complex param types passed through."""
        stub = StubExecuteClient(["id"], [[1]])

        params = [
            42,
            "string",
            3.14,
            True,
            None,
            ["list", "of", "values"],
            {"key": "value"},
        ]
        await execute_raw("SELECT ...", params, client=stub)

        assert stub.calls[0]["params"] == params

    @pytest.mark.asyncio
    async def test_empty_sql(self, reset_state):
        """Empty SQL string passed through (db will error)."""
        stub = StubExecuteClient(["x"], [[1]])

        await execute_raw("", client=stub)

        assert stub.calls[0]["sql"] == ""

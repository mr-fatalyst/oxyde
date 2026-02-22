"""Tests for connection pool lifecycle: connect, disconnect, registry."""

from __future__ import annotations

from datetime import timedelta

import pytest

from oxyde.db.pool import (
    AsyncDatabase,
    PoolSettings,
    _msgpack_encoder,
    _normalize_duration,
    _validate_url_scheme,
)
from oxyde.db.registry import (
    _CONNECTIONS,
    disconnect_all,
    get_connection,
    register_connection,
)


@pytest.fixture(autouse=True)
def reset_connections():
    """Reset connection registry before each test."""
    _CONNECTIONS.clear()
    yield
    _CONNECTIONS.clear()


class TestPoolSettings:
    """Test PoolSettings configuration."""

    def test_default_settings(self):
        """Test PoolSettings default values."""
        settings = PoolSettings()

        assert settings.max_connections is None
        assert settings.min_connections is None
        assert settings.sqlite_journal_mode == "WAL"
        assert settings.sqlite_synchronous == "NORMAL"
        assert settings.sqlite_cache_size == 10000
        assert settings.sqlite_busy_timeout == 5000

    def test_custom_settings(self):
        """Test PoolSettings with custom values."""
        settings = PoolSettings(
            max_connections=10,
            min_connections=2,
            acquire_timeout=5.0,
            idle_timeout=300,
        )

        assert settings.max_connections == 10
        assert settings.min_connections == 2
        assert settings.acquire_timeout == 5.0
        assert settings.idle_timeout == 300

    def test_to_payload_empty(self):
        """Test to_payload() with defaults returns minimal payload."""
        settings = PoolSettings()
        payload = settings.to_payload()

        # Should include default SQLite settings
        assert payload is not None
        assert payload["sqlite_journal_mode"] == "WAL"

    def test_to_payload_with_values(self):
        """Test to_payload() with custom values."""
        settings = PoolSettings(
            max_connections=20,
            min_connections=5,
            acquire_timeout=10.0,
        )
        payload = settings.to_payload()

        assert payload["max_connections"] == 20
        assert payload["min_connections"] == 5
        assert payload["acquire_timeout"] == 10.0

    def test_to_payload_timedelta_conversion(self):
        """Test to_payload() converts timedelta to float."""
        settings = PoolSettings(
            acquire_timeout=timedelta(seconds=5),
            idle_timeout=timedelta(minutes=5),
        )
        payload = settings.to_payload()

        assert payload["acquire_timeout"] == 5.0
        assert payload["idle_timeout"] == 300.0

    def test_to_payload_sqlite_settings(self):
        """Test to_payload() includes SQLite settings."""
        settings = PoolSettings(
            sqlite_journal_mode="DELETE",
            sqlite_synchronous="FULL",
            sqlite_cache_size=5000,
            sqlite_busy_timeout=10000,
        )
        payload = settings.to_payload()

        assert payload["sqlite_journal_mode"] == "DELETE"
        assert payload["sqlite_synchronous"] == "FULL"
        assert payload["sqlite_cache_size"] == 5000
        assert payload["sqlite_busy_timeout"] == 10000

    def test_to_payload_transaction_settings(self):
        """Test to_payload() includes transaction settings."""
        settings = PoolSettings(
            transaction_timeout=600,
            transaction_cleanup_interval=120,
        )
        payload = settings.to_payload()

        assert payload["transaction_timeout"] == 600.0
        assert payload["transaction_cleanup_interval"] == 120.0


class TestNormalizeDuration:
    """Test _normalize_duration helper function."""

    def test_normalize_none(self):
        """Test normalizing None."""
        assert _normalize_duration(None) is None

    def test_normalize_int(self):
        """Test normalizing int."""
        assert _normalize_duration(10) == 10.0

    def test_normalize_float(self):
        """Test normalizing float."""
        assert _normalize_duration(5.5) == 5.5

    def test_normalize_timedelta(self):
        """Test normalizing timedelta."""
        assert _normalize_duration(timedelta(seconds=30)) == 30.0
        assert _normalize_duration(timedelta(minutes=1)) == 60.0
        assert _normalize_duration(timedelta(hours=1)) == 3600.0

    def test_normalize_invalid_type_raises(self):
        """Test that invalid type raises TypeError."""
        with pytest.raises(TypeError):
            _normalize_duration("invalid")


class TestValidateUrlScheme:
    """Test _validate_url_scheme helper function."""

    def test_valid_postgres_urls(self):
        """Test valid PostgreSQL URLs."""
        _validate_url_scheme("postgres://user:pass@host/db")
        _validate_url_scheme("postgresql://user:pass@host/db")
        # Should not raise

    def test_valid_mysql_urls(self):
        """Test valid MySQL URLs."""
        _validate_url_scheme("mysql://user:pass@host/db")
        # Should not raise

    def test_valid_sqlite_urls(self):
        """Test valid SQLite URLs."""
        _validate_url_scheme("sqlite:///path/to/db.sqlite")
        _validate_url_scheme("sqlite:///:memory:")
        # Should not raise

    def test_invalid_url_raises(self):
        """Test invalid URL scheme raises error."""
        with pytest.raises(ValueError):
            _validate_url_scheme("mongodb://host/db")

        with pytest.raises(ValueError):
            _validate_url_scheme("redis://host")


class TestMsgpackEncoder:
    """Test _msgpack_encoder for msgpack serialization."""

    def test_encode_datetime(self):
        """Test encoding datetime."""
        from datetime import datetime

        dt = datetime(2024, 1, 15, 12, 30, 45)
        result = _msgpack_encoder(dt)
        assert result == "2024-01-15T12:30:45"

    def test_encode_date(self):
        """Test encoding date (was BUG-2)."""
        from datetime import date

        d = date(2024, 1, 15)
        result = _msgpack_encoder(d)
        assert result == "2024-01-15"

    def test_encode_uuid(self):
        """Test encoding UUID (was BUG-2)."""
        from uuid import UUID

        u = UUID("12345678-1234-5678-1234-567812345678")
        result = _msgpack_encoder(u)
        assert result == "12345678-1234-5678-1234-567812345678"

    def test_encode_decimal(self):
        """Test encoding Decimal (was BUG-2)."""
        from decimal import Decimal

        d = Decimal("3.14")
        result = _msgpack_encoder(d)
        assert result == "3.14"

    def test_encode_native_types_pass_through(self):
        """Test that native msgpack types pass through unchanged."""
        assert _msgpack_encoder("test") == "test"
        assert _msgpack_encoder(42) == 42
        assert _msgpack_encoder(3.14) == 3.14
        assert _msgpack_encoder(True) is True


class TestAsyncDatabaseInit:
    """Test AsyncDatabase initialization."""

    def test_init_with_valid_url(self):
        """Test initialization with valid URL."""
        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            auto_register=False,
        )

        assert db.url == "sqlite:///test.db"
        assert db.name == "test"
        assert db.connected is False

    def test_init_with_invalid_url_raises(self):
        """Test initialization with invalid URL raises error."""
        with pytest.raises(ValueError):
            AsyncDatabase(
                url="invalid://host/db",
                name="test",
                auto_register=False,
            )

    def test_init_auto_registers(self):
        """Test initialization auto-registers by default."""
        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="auto_test",
        )

        assert "auto_test" in _CONNECTIONS

    def test_init_with_settings(self):
        """Test initialization with custom settings."""
        settings = PoolSettings(max_connections=5)
        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            settings=settings,
            auto_register=False,
        )

        assert db.settings.max_connections == 5


class TestAsyncDatabaseConnect:
    """Test AsyncDatabase connect/disconnect lifecycle."""

    @pytest.mark.asyncio
    async def test_connect_sets_connected_flag(self, monkeypatch):
        """Test connect() sets connected flag."""

        async def mock_init_pool(name, url, settings):
            pass

        monkeypatch.setattr("oxyde.db.pool._init_pool", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool._init_pool_overwrite", mock_init_pool)

        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            auto_register=False,
        )

        assert db.connected is False
        await db.connect()
        assert db.connected is True

    @pytest.mark.asyncio
    async def test_connect_is_idempotent(self, monkeypatch):
        """Test multiple connect() calls are idempotent."""
        call_count = 0

        async def mock_init_pool(name, url, settings):
            nonlocal call_count
            call_count += 1

        monkeypatch.setattr("oxyde.db.pool._init_pool", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool._init_pool_overwrite", mock_init_pool)

        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            auto_register=False,
        )

        await db.connect()
        await db.connect()
        await db.connect()

        assert call_count == 1  # Only called once

    @pytest.mark.asyncio
    async def test_disconnect_clears_connected_flag(self, monkeypatch):
        """Test disconnect() clears connected flag."""

        async def mock_init_pool(name, url, settings):
            pass

        async def mock_close_pool(name):
            pass

        monkeypatch.setattr("oxyde.db.pool._init_pool", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool._init_pool_overwrite", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool.close_pool", mock_close_pool)

        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            auto_register=False,
        )

        await db.connect()
        assert db.connected is True

        await db.disconnect()
        assert db.connected is False

    @pytest.mark.asyncio
    async def test_disconnect_is_idempotent(self, monkeypatch):
        """Test multiple disconnect() calls are idempotent."""
        call_count = 0

        async def mock_init_pool(name, url, settings):
            pass

        async def mock_close_pool(name):
            nonlocal call_count
            call_count += 1

        monkeypatch.setattr("oxyde.db.pool._init_pool", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool._init_pool_overwrite", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool.close_pool", mock_close_pool)

        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            auto_register=False,
        )

        await db.connect()
        await db.disconnect()
        await db.disconnect()
        await db.disconnect()

        assert call_count == 1  # Only called once


class TestAsyncDatabaseContextManager:
    """Test AsyncDatabase as context manager."""

    @pytest.mark.asyncio
    async def test_context_manager_connects_and_disconnects(self, monkeypatch):
        """Test context manager connects on enter and disconnects on exit."""
        connected = False
        disconnected = False

        async def mock_init_pool(name, url, settings):
            nonlocal connected
            connected = True

        async def mock_close_pool(name):
            nonlocal disconnected
            disconnected = True

        monkeypatch.setattr("oxyde.db.pool._init_pool", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool._init_pool_overwrite", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool.close_pool", mock_close_pool)

        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            auto_register=False,
        )

        async with db:
            assert connected is True
            assert disconnected is False

        assert disconnected is True


class TestAsyncDatabaseExecute:
    """Test AsyncDatabase.execute() method."""

    @pytest.mark.asyncio
    async def test_execute_requires_connection(self):
        """Test execute() raises error if not connected."""
        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            auto_register=False,
        )

        with pytest.raises(RuntimeError, match="not connected"):
            await db.execute({"op": "select"})


class TestConnectionRegistry:
    """Test connection registry functions."""

    def test_register_connection(self):
        """Test register_connection()."""
        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test_reg",
            auto_register=False,
        )

        register_connection(db)

        assert "test_reg" in _CONNECTIONS
        assert _CONNECTIONS["test_reg"] is db

    def test_register_connection_overwrite(self):
        """Test register_connection() with overwrite."""
        db1 = AsyncDatabase(
            url="sqlite:///test1.db",
            name="test",
            auto_register=False,
        )
        db2 = AsyncDatabase(
            url="sqlite:///test2.db",
            name="test",
            auto_register=False,
        )

        register_connection(db1)
        register_connection(db2, overwrite=True)

        assert _CONNECTIONS["test"] is db2

    def test_register_connection_no_overwrite_raises(self):
        """Test register_connection() without overwrite raises on duplicate."""
        db1 = AsyncDatabase(
            url="sqlite:///test1.db",
            name="test",
            auto_register=False,
        )
        db2 = AsyncDatabase(
            url="sqlite:///test2.db",
            name="test",
            auto_register=False,
        )

        register_connection(db1)

        with pytest.raises(ValueError):
            register_connection(db2, overwrite=False)

    @pytest.mark.asyncio
    async def test_get_connection(self, monkeypatch):
        """Test get_connection()."""

        async def mock_init_pool(name, url, settings):
            pass

        monkeypatch.setattr("oxyde.db.pool._init_pool", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool._init_pool_overwrite", mock_init_pool)

        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test_get",
        )

        retrieved = await get_connection("test_get")

        assert retrieved is db

    @pytest.mark.asyncio
    async def test_get_connection_not_found_raises(self):
        """Test get_connection() raises for unknown name."""
        with pytest.raises(KeyError):
            await get_connection("nonexistent")

    @pytest.mark.asyncio
    async def test_disconnect_all(self, monkeypatch):
        """Test disconnect_all()."""
        close_all_called = False

        async def mock_init_pool(name, url, settings):
            pass

        async def mock_close_all_pools():
            nonlocal close_all_called
            close_all_called = True

        monkeypatch.setattr("oxyde.db.pool._init_pool", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool._init_pool_overwrite", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool.close_all_pools", mock_close_all_pools)
        monkeypatch.setattr("oxyde.db.registry.close_all_pools", mock_close_all_pools)

        db1 = AsyncDatabase(url="sqlite:///test1.db", name="db1")
        db2 = AsyncDatabase(url="sqlite:///test2.db", name="db2")

        await db1.connect()
        await db2.connect()

        assert db1.connected is True
        assert db2.connected is True

        await disconnect_all()

        # disconnect_all() marks connections as disconnected and calls close_all_pools()
        assert db1.connected is False
        assert db2.connected is False
        assert close_all_called is True


class TestEnsureConnected:
    """Test ensure_connected() method."""

    @pytest.mark.asyncio
    async def test_ensure_connected_connects_if_not_connected(self, monkeypatch):
        """Test ensure_connected() connects if not already connected."""
        call_count = 0

        async def mock_init_pool(name, url, settings):
            nonlocal call_count
            call_count += 1

        monkeypatch.setattr("oxyde.db.pool._init_pool", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool._init_pool_overwrite", mock_init_pool)

        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            auto_register=False,
        )

        await db.ensure_connected()

        assert call_count == 1
        assert db.connected is True

    @pytest.mark.asyncio
    async def test_ensure_connected_noop_if_connected(self, monkeypatch):
        """Test ensure_connected() is no-op if already connected."""
        call_count = 0

        async def mock_init_pool(name, url, settings):
            nonlocal call_count
            call_count += 1

        monkeypatch.setattr("oxyde.db.pool._init_pool", mock_init_pool)
        monkeypatch.setattr("oxyde.db.pool._init_pool_overwrite", mock_init_pool)

        db = AsyncDatabase(
            url="sqlite:///test.db",
            name="test",
            auto_register=False,
        )

        await db.connect()
        await db.ensure_connected()
        await db.ensure_connected()

        assert call_count == 1  # Only connected once

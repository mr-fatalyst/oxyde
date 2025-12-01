"""Tests for high-level db API: init, close, connect, lifespan."""

from __future__ import annotations

import pytest
import pytest_asyncio

from oxyde import db
from oxyde.db import PoolSettings


@pytest_asyncio.fixture(autouse=True, loop_scope="function")
async def cleanup_connections():
    """Clean up connections before and after each test."""
    await db.close()
    yield
    await db.close()


class TestDbInit:
    """Test db.init() function."""

    @pytest.mark.asyncio
    async def test_init_single_database(self):
        """Test initializing a single database."""
        await db.init(default="sqlite::memory:")

        # Should be able to get connection
        conn = await db.get_connection("default")
        assert conn.connected
        assert conn.name == "default"

    @pytest.mark.asyncio
    async def test_init_multiple_databases(self):
        """Test initializing multiple databases."""
        await db.init(
            default="sqlite::memory:",
            analytics="sqlite::memory:",
        )

        conn1 = await db.get_connection("default")
        conn2 = await db.get_connection("analytics")

        assert conn1.connected
        assert conn2.connected
        assert conn1.name == "default"
        assert conn2.name == "analytics"

    @pytest.mark.asyncio
    async def test_init_with_settings(self):
        """Test initializing with custom settings."""
        settings = PoolSettings(max_connections=5)
        await db.init(
            default="sqlite::memory:",
            settings=settings,
        )

        conn = await db.get_connection("default")
        assert conn.connected
        assert conn.settings.max_connections == 5

    @pytest.mark.asyncio
    async def test_init_no_databases_raises(self):
        """Test that init without databases raises ValueError."""
        with pytest.raises(ValueError, match="At least one database"):
            await db.init()

    @pytest.mark.asyncio
    async def test_init_overwrites_existing(self):
        """Test that init overwrites existing connections."""
        await db.init(default="sqlite::memory:")
        conn1 = await db.get_connection("default")

        # Init again - should overwrite
        await db.init(default="sqlite::memory:")
        conn2 = await db.get_connection("default")

        # Should be different instances (overwritten)
        assert conn1 is not conn2


class TestDbClose:
    """Test db.close() function."""

    @pytest.mark.asyncio
    async def test_close_disconnects_all(self):
        """Test that close disconnects all connections."""
        await db.init(
            default="sqlite::memory:",
            analytics="sqlite::memory:",
        )

        conn1 = await db.get_connection("default")
        conn2 = await db.get_connection("analytics")

        assert conn1.connected
        assert conn2.connected

        await db.close()

        assert not conn1.connected
        assert not conn2.connected

    @pytest.mark.asyncio
    async def test_close_when_not_connected(self):
        """Test that close works even when no connections exist."""
        # Should not raise
        await db.close()


class TestDbConnect:
    """Test db.connect() context manager."""

    @pytest.mark.asyncio
    async def test_connect_context_manager(self):
        """Test connect as context manager."""
        async with db.connect("sqlite::memory:") as conn:
            assert conn.connected
            assert conn.name == "default"

        # After exit, should be disconnected
        assert not conn.connected

    @pytest.mark.asyncio
    async def test_connect_with_custom_name(self):
        """Test connect with custom name."""
        async with db.connect("sqlite::memory:", name="test_db") as conn:
            assert conn.name == "test_db"

            # Should be accessible via get_connection
            retrieved = await db.get_connection("test_db")
            assert retrieved is conn

    @pytest.mark.asyncio
    async def test_connect_with_settings(self):
        """Test connect with custom settings."""
        settings = PoolSettings(max_connections=3)
        async with db.connect("sqlite::memory:", settings=settings) as conn:
            assert conn.settings.max_connections == 3

    @pytest.mark.asyncio
    async def test_connect_cleanup_on_exception(self):
        """Test that connect cleans up on exception."""
        conn_ref = None

        with pytest.raises(RuntimeError):
            async with db.connect("sqlite::memory:") as conn:
                conn_ref = conn
                raise RuntimeError("Test error")

        # Should be disconnected even after exception
        assert not conn_ref.connected


class TestDbLifespan:
    """Test db.lifespan() for FastAPI."""

    @pytest.mark.asyncio
    async def test_lifespan_basic(self):
        """Test lifespan context manager."""
        lifespan_cm = db.lifespan(default="sqlite::memory:")

        # Simulate FastAPI lifespan
        async with lifespan_cm(None):  # app=None for testing
            conn = await db.get_connection("default")
            assert conn.connected

        # After exit
        assert not conn.connected

    @pytest.mark.asyncio
    async def test_lifespan_multiple_databases(self):
        """Test lifespan with multiple databases."""
        lifespan_cm = db.lifespan(
            default="sqlite::memory:",
            analytics="sqlite::memory:",
        )

        async with lifespan_cm(None):
            conn1 = await db.get_connection("default")
            conn2 = await db.get_connection("analytics")
            assert conn1.connected
            assert conn2.connected

        assert not conn1.connected
        assert not conn2.connected

    @pytest.mark.asyncio
    async def test_lifespan_with_settings(self):
        """Test lifespan with custom settings."""
        settings = PoolSettings(max_connections=10)
        lifespan_cm = db.lifespan(
            default="sqlite::memory:",
            settings=settings,
        )

        async with lifespan_cm(None):
            conn = await db.get_connection("default")
            assert conn.settings.max_connections == 10


class TestDbIntegration:
    """Integration tests for db API."""

    @pytest.mark.asyncio
    async def test_full_workflow(self):
        """Test full workflow: init, use, close."""
        # Init
        await db.init(default="sqlite::memory:")

        # Use
        conn = await db.get_connection("default")
        assert conn.connected

        # Close
        await db.close()
        assert not conn.connected

    @pytest.mark.asyncio
    async def test_reinit_after_close(self):
        """Test that we can reinit after close."""
        await db.init(default="sqlite::memory:")
        await db.close()

        # Should be able to init again
        await db.init(default="sqlite::memory:")
        conn = await db.get_connection("default")
        assert conn.connected

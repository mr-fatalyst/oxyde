"""Tests for row-level locking: for_update(), for_share()."""

from __future__ import annotations

import pytest

from oxyde import Field, OxydeModel
from oxyde.models.registry import clear_registry


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


class Account(OxydeModel):
    """Test model for locking tests."""

    id: int | None = Field(default=None, db_pk=True)
    name: str
    balance: int = 0

    class Meta:
        is_table = True


class TestForUpdate:
    """Test for_update() method."""

    def test_for_update_sets_lock_type(self):
        """Test for_update() sets _lock_type to 'update'."""
        query = Account.objects.filter(id=1).for_update()
        assert query._lock_type == "update"

    def test_for_update_chainable(self):
        """Test for_update() is chainable with other methods."""
        query = (
            Account.objects
            .filter(balance__gte=100)
            .for_update()
            .order_by("name")
            .limit(10)
        )
        assert query._lock_type == "update"
        assert query._limit_value == 10

    def test_for_update_in_ir(self):
        """Test for_update() includes lock in IR."""
        query = Account.objects.filter(id=1).for_update()
        ir = query.to_ir()
        assert ir.get("lock") == "update"

    def test_for_update_from_manager(self):
        """Test for_update() can be called from manager."""
        query = Account.objects.for_update().filter(id=1)
        assert query._lock_type == "update"

    def test_for_update_immutable(self):
        """Test for_update() returns new query, doesn't modify original."""
        original = Account.objects.filter(id=1)
        with_lock = original.for_update()

        assert original._lock_type is None
        assert with_lock._lock_type == "update"


class TestForShare:
    """Test for_share() method."""

    def test_for_share_sets_lock_type(self):
        """Test for_share() sets _lock_type to 'share'."""
        query = Account.objects.filter(id=1).for_share()
        assert query._lock_type == "share"

    def test_for_share_chainable(self):
        """Test for_share() is chainable with other methods."""
        query = (
            Account.objects
            .filter(balance__gte=0)
            .for_share()
            .limit(100)
        )
        assert query._lock_type == "share"

    def test_for_share_in_ir(self):
        """Test for_share() includes lock in IR."""
        query = Account.objects.filter(id=1).for_share()
        ir = query.to_ir()
        assert ir.get("lock") == "share"

    def test_for_share_from_manager(self):
        """Test for_share() can be called from manager."""
        query = Account.objects.for_share().filter(id=1)
        assert query._lock_type == "share"


class TestLockingIR:
    """Test IR generation for locking."""

    def test_no_lock_by_default(self):
        """Test queries have no lock by default."""
        query = Account.objects.filter(id=1)
        ir = query.to_ir()
        assert "lock" not in ir

    def test_lock_preserved_through_chain(self):
        """Test lock type is preserved through query chaining."""
        query = (
            Account.objects
            .for_update()
            .filter(id=1)
            .order_by("name")
            .limit(1)
        )
        ir = query.to_ir()
        assert ir.get("lock") == "update"

    def test_last_lock_wins(self):
        """Test calling for_share() after for_update() uses share."""
        query = Account.objects.for_update().for_share()
        assert query._lock_type == "share"
        ir = query.to_ir()
        assert ir.get("lock") == "share"

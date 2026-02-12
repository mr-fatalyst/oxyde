"""Tests for QueryManager CRUD operations: save, delete, get_or_create, bulk operations."""

from __future__ import annotations

from typing import Any

import msgpack
import pytest

from oxyde import Field, Model
from oxyde.exceptions import (
    FieldError,
    ManagerError,
    MultipleObjectsReturned,
    NotFoundError,
)
from oxyde.models.registry import clear_registry


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


class StubExecuteClient:
    """Stub client for testing - returns msgpack encoded data."""

    def __init__(self, payloads: list):
        self.payloads = list(payloads)
        self.calls: list[dict[str, Any]] = []

    async def execute(self, ir: dict[str, Any]) -> bytes:
        self.calls.append(ir)
        if not self.payloads:
            raise RuntimeError("stub payloads exhausted")
        payload = self.payloads.pop(0)
        if isinstance(payload, bytes):
            return payload
        return msgpack.packb(payload)


class TestModel(Model):
    """Test model for CRUD operations."""

    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str | None = None
    age: int = 0
    is_active: bool = True

    class Meta:
        is_table = True


class TestModelSave:
    """Test Model.save() instance method."""

    @pytest.mark.asyncio
    async def test_save_new_instance_creates(self):
        """Test save() on new instance (no PK) creates record."""
        stub = StubExecuteClient([{
            "columns": ["id", "name", "email", "age", "is_active"],
            "rows": [[42, "Alice", None, 25, True]]
        }])

        instance = TestModel(name="Alice", age=25)
        result = await instance.save(client=stub)

        assert stub.calls[0]["op"] == "insert"
        assert result is instance
        assert instance.id == 42

    @pytest.mark.asyncio
    async def test_save_existing_instance_updates(self):
        """Test save() on existing instance (with PK) updates record."""
        stub = StubExecuteClient([{
            "columns": ["id", "name", "email", "age", "is_active"],
            "rows": [[1, "Bob", None, 30, True]]
        }])

        instance = TestModel(id=1, name="Bob", age=30)
        result = await instance.save(client=stub)

        assert stub.calls[0]["op"] == "update"
        assert result is instance

    @pytest.mark.asyncio
    async def test_save_with_update_fields(self):
        """Test save() with update_fields updates only specified fields."""
        stub = StubExecuteClient([{
            "columns": ["id", "name", "email", "age", "is_active"],
            "rows": [[1, "Charlie", "c@example.com", 35, True]]
        }])

        instance = TestModel(id=1, name="Charlie", email="c@example.com", age=35)
        await instance.save(client=stub, update_fields=["name", "age"])

        call = stub.calls[0]
        assert call["op"] == "update"
        assert "name" in call["values"]
        assert "age" in call["values"]
        # email should not be in values since it's not in update_fields
        assert "email" not in call["values"]

    @pytest.mark.asyncio
    async def test_save_update_fields_validates_field_names(self):
        """Test save() with invalid field names raises error."""
        stub = StubExecuteClient([{"affected": 1}])

        instance = TestModel(id=1, name="Test")

        with pytest.raises(FieldError):
            await instance.save(client=stub, update_fields=["nonexistent"])

    @pytest.mark.asyncio
    async def test_save_update_not_found_raises(self):
        """Test save() raises NotFoundError when record not found."""
        stub = StubExecuteClient([{"affected": 0}])

        instance = TestModel(id=999, name="Ghost")

        with pytest.raises(NotFoundError):
            await instance.save(client=stub)


class TestModelDelete:
    """Test Model.delete() instance method."""

    @pytest.mark.asyncio
    async def test_delete_removes_record(self):
        """Test delete() removes record from database."""
        stub = StubExecuteClient([{"affected": 1}])

        instance = TestModel(id=1, name="ToDelete")
        affected = await instance.delete(client=stub)

        assert stub.calls[0]["op"] == "delete"
        assert affected == 1

    @pytest.mark.asyncio
    async def test_delete_without_pk_raises(self):
        """Test delete() without PK value raises error."""
        stub = StubExecuteClient([])

        instance = TestModel(name="NoPK")
        instance.id = None

        with pytest.raises(ManagerError):
            await instance.delete(client=stub)

    @pytest.mark.asyncio
    async def test_delete_returns_affected_count(self):
        """Test delete() returns number of affected rows."""
        stub = StubExecuteClient([{"affected": 0}])

        instance = TestModel(id=999, name="NotFound")
        affected = await instance.delete(client=stub)

        assert affected == 0


class TestModelRefresh:
    """Test Model.refresh() instance method."""

    @pytest.mark.asyncio
    async def test_refresh_reloads_from_db(self):
        """Test refresh() reloads instance data from database."""
        stub = StubExecuteClient(
            [
                [
                    {
                        "id": 1,
                        "name": "Updated",
                        "email": None,
                        "age": 99,
                        "is_active": True,
                    }
                ]
            ]
        )

        instance = TestModel(id=1, name="Original", age=25)
        await instance.refresh(client=stub)

        assert instance.name == "Updated"
        assert instance.age == 99

    @pytest.mark.asyncio
    async def test_refresh_without_pk_raises(self):
        """Test refresh() without PK value raises error."""
        stub = StubExecuteClient([])

        instance = TestModel(name="NoPK")

        with pytest.raises(ManagerError):
            await instance.refresh(client=stub)


class TestManagerCreate:
    """Test QueryManager.create() method."""

    @pytest.mark.asyncio
    async def test_create_with_kwargs(self):
        """Test create() with keyword arguments."""
        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])

        instance = await TestModel.objects.create(client=stub, name="NewUser", age=25)

        assert isinstance(instance, TestModel)
        assert instance.name == "NewUser"
        assert stub.calls[0]["op"] == "insert"

    @pytest.mark.asyncio
    async def test_create_with_instance(self):
        """Test create() with model instance."""
        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])

        obj = TestModel(name="FromInstance", age=30)
        instance = await TestModel.objects.create(client=stub, instance=obj)

        assert instance is obj
        assert stub.calls[0]["op"] == "insert"

    @pytest.mark.asyncio
    async def test_create_requires_data(self):
        """Test create() without data raises error."""
        stub = StubExecuteClient([])

        with pytest.raises(ManagerError):
            await TestModel.objects.create(client=stub)

    @pytest.mark.asyncio
    async def test_create_instance_and_kwargs_raises(self):
        """Test create() with both instance and kwargs raises error."""
        stub = StubExecuteClient([])

        with pytest.raises(ManagerError):
            await TestModel.objects.create(
                client=stub, instance=TestModel(name="Test"), name="Other"
            )


class TestManagerBulkCreate:
    """Test QueryManager.bulk_create() method."""

    @pytest.mark.asyncio
    async def test_bulk_create_with_dicts(self):
        """Test bulk_create() with dict objects."""
        stub = StubExecuteClient([{"affected": 2, "inserted_ids": [1, 2]}])

        objects = [
            {"name": "User1", "age": 20},
            {"name": "User2", "age": 30},
        ]
        created = await TestModel.objects.bulk_create(objects, client=stub)

        assert len(created) == 2
        assert stub.calls[0]["op"] == "insert"

    @pytest.mark.asyncio
    async def test_bulk_create_with_instances(self):
        """Test bulk_create() with model instances."""
        stub = StubExecuteClient([{"affected": 2, "inserted_ids": [1, 2]}])

        objects = [
            TestModel(name="User1", age=20),
            TestModel(name="User2", age=30),
        ]
        created = await TestModel.objects.bulk_create(objects, client=stub)

        assert len(created) == 2
        assert created[0].id == 1
        assert created[1].id == 2

    @pytest.mark.asyncio
    async def test_bulk_create_mixed(self):
        """Test bulk_create() with mixed dicts and instances."""
        stub = StubExecuteClient([{"affected": 2, "inserted_ids": [1, 2]}])

        objects = [
            {"name": "Dict", "age": 20},
            TestModel(name="Instance", age=30),
        ]
        created = await TestModel.objects.bulk_create(objects, client=stub)

        assert len(created) == 2

    @pytest.mark.asyncio
    async def test_bulk_create_empty_list(self):
        """Test bulk_create() with empty list returns empty."""
        stub = StubExecuteClient([])

        created = await TestModel.objects.bulk_create([], client=stub)

        assert created == []
        assert len(stub.calls) == 0


class TestManagerBulkUpdate:
    """Test QueryManager.bulk_update() method."""

    @pytest.mark.asyncio
    async def test_bulk_update_updates_records(self):
        """Test bulk_update() updates multiple records."""
        stub = StubExecuteClient([{"affected": 2}])

        objects = [
            TestModel(id=1, name="Updated1", age=25),
            TestModel(id=2, name="Updated2", age=35),
        ]
        affected = await TestModel.objects.bulk_update(
            objects, ["name", "age"], client=stub
        )

        assert affected == 2

    @pytest.mark.asyncio
    async def test_bulk_update_requires_fields(self):
        """Test bulk_update() requires at least one field."""
        stub = StubExecuteClient([])

        objects = [TestModel(id=1, name="Test")]

        with pytest.raises(ManagerError):
            await TestModel.objects.bulk_update(objects, [], client=stub)

    @pytest.mark.asyncio
    async def test_bulk_update_empty_list(self):
        """Test bulk_update() with empty list returns 0."""
        stub = StubExecuteClient([])

        affected = await TestModel.objects.bulk_update([], ["name"], client=stub)

        assert affected == 0


class TestManagerGetOrCreate:
    """Test QueryManager.get_or_create() method."""

    @pytest.mark.asyncio
    async def test_get_or_create_finds_existing(self):
        """Test get_or_create() finds existing record."""
        stub = StubExecuteClient(
            [
                [
                    {
                        "id": 1,
                        "name": "Existing",
                        "email": None,
                        "age": 25,
                        "is_active": True,
                    }
                ]
            ]
        )

        obj, created = await TestModel.objects.get_or_create(
            client=stub, name="Existing"
        )

        assert created is False
        assert isinstance(obj, TestModel)
        assert obj.id == 1

    @pytest.mark.asyncio
    async def test_get_or_create_creates_new(self):
        """Test get_or_create() creates new record when not found."""
        stub = StubExecuteClient(
            [
                [],  # First query returns empty
                {"affected": 1, "inserted_ids": [42]},  # Then insert
            ]
        )

        obj, created = await TestModel.objects.get_or_create(
            client=stub, defaults={"age": 30}, name="NewUser"
        )

        assert created is True
        assert isinstance(obj, TestModel)
        assert obj.name == "NewUser"

    @pytest.mark.asyncio
    async def test_get_or_create_uses_defaults(self):
        """Test get_or_create() uses defaults for new record."""
        stub = StubExecuteClient([[], {"affected": 1, "inserted_ids": [1]}])

        obj, created = await TestModel.objects.get_or_create(
            client=stub,
            defaults={"age": 99, "email": "test@example.com"},
            name="WithDefaults",
        )

        assert created is True
        # The insert call should include defaults
        insert_call = stub.calls[1]
        assert insert_call["values"]["age"] == 99


class TestManagerGet:
    """Test QueryManager.get() method."""

    @pytest.mark.asyncio
    async def test_get_finds_single_record(self):
        """Test get() finds single matching record."""
        stub = StubExecuteClient(
            [[{"id": 1, "name": "Found", "email": None, "age": 25, "is_active": True}]]
        )

        obj = await TestModel.objects.get(client=stub, name="Found")

        assert isinstance(obj, TestModel)
        assert obj.name == "Found"

    @pytest.mark.asyncio
    async def test_get_not_found_raises(self):
        """Test get() raises NotFoundError when no match."""
        stub = StubExecuteClient([[]])

        with pytest.raises(NotFoundError):
            await TestModel.objects.get(client=stub, name="Missing")

    @pytest.mark.asyncio
    async def test_get_multiple_raises(self):
        """Test get() raises MultipleObjectsReturned when multiple matches."""
        stub = StubExecuteClient(
            [
                [
                    {
                        "id": 1,
                        "name": "Dup",
                        "email": None,
                        "age": 25,
                        "is_active": True,
                    },
                    {
                        "id": 2,
                        "name": "Dup",
                        "email": None,
                        "age": 30,
                        "is_active": True,
                    },
                ]
            ]
        )

        with pytest.raises(MultipleObjectsReturned):
            await TestModel.objects.get(client=stub, name="Dup")


class TestManagerGetOrNone:
    """Test QueryManager.get_or_none() method."""

    @pytest.mark.asyncio
    async def test_get_or_none_finds_record(self):
        """Test get_or_none() finds matching record."""
        stub = StubExecuteClient(
            [[{"id": 1, "name": "Found", "email": None, "age": 25, "is_active": True}]]
        )

        obj = await TestModel.objects.get_or_none(client=stub, name="Found")

        assert obj is not None
        assert obj.name == "Found"

    @pytest.mark.asyncio
    async def test_get_or_none_returns_none_when_not_found(self):
        """Test get_or_none() returns None when no match."""
        stub = StubExecuteClient([[]])

        obj = await TestModel.objects.get_or_none(client=stub, name="Missing")

        assert obj is None


class TestManagerDelete:
    """Test QueryManager.delete() method."""

    @pytest.mark.asyncio
    async def test_filter_delete(self):
        """Test delete() through filter."""
        stub = StubExecuteClient([{"affected": 5}])

        affected = await TestModel.objects.filter(is_active=False).delete(client=stub)

        assert affected == 5
        assert stub.calls[0]["op"] == "delete"


class TestManagerUpdate:
    """Test QueryManager.update() method."""

    @pytest.mark.asyncio
    async def test_filter_update(self):
        """Test update() through filter."""
        stub = StubExecuteClient([{
            "columns": ["id", "name", "age"],
            "rows": [[1, "a", 25], [2, "b", 25], [3, "c", 25]]
        }])

        # update() takes keyword arguments, not positional dict
        rows = await TestModel.objects.filter(is_active=True).update(
            age=25, client=stub
        )

        assert len(rows) == 3
        assert stub.calls[0]["op"] == "update"


class TestManagerFirstLast:
    """Test QueryManager.first() and last() methods."""

    @pytest.mark.asyncio
    async def test_first_returns_first_record(self):
        """Test first() returns first record ordered by PK."""
        stub = StubExecuteClient(
            [[{"id": 1, "name": "First", "email": None, "age": 20, "is_active": True}]]
        )

        obj = await TestModel.objects.first(client=stub)

        assert obj is not None
        assert obj.id == 1

    @pytest.mark.asyncio
    async def test_first_returns_none_when_empty(self):
        """Test first() returns None when no records."""
        stub = StubExecuteClient([[]])

        obj = await TestModel.objects.first(client=stub)

        assert obj is None

    @pytest.mark.asyncio
    async def test_last_returns_last_record(self):
        """Test last() returns last record ordered by -PK."""
        stub = StubExecuteClient(
            [[{"id": 99, "name": "Last", "email": None, "age": 50, "is_active": True}]]
        )

        obj = await TestModel.objects.last(client=stub)

        assert obj is not None
        assert obj.id == 99


class TestManagerCount:
    """Test QueryManager.count() method."""

    @pytest.mark.asyncio
    async def test_count_returns_count(self):
        """Test count() returns record count."""
        # count() expects aggregate result with _count field
        stub = StubExecuteClient([[{"_count": 3}]])

        count = await TestModel.objects.count(client=stub)

        assert count == 3


class TestManagerUpsert:
    """Test QueryManager.upsert() placeholder."""

    @pytest.mark.asyncio
    async def test_upsert_raises_not_implemented(self):
        """Test upsert() raises ManagerError (not implemented)."""
        with pytest.raises(ManagerError):
            await TestModel.objects.upsert()

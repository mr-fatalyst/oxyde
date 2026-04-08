"""Tests for QueryManager CRUD operations: save, delete, get_or_create, bulk operations."""

from __future__ import annotations

import pytest
from pydantic import computed_field

from oxyde import Field, Model
from oxyde.exceptions import (
    FieldError,
    IntegrityError,
    ManagerError,
    MultipleObjectsReturned,
    NotFoundError,
)
from oxyde.tests.helpers import StubExecuteClient


class OxydeTestModel(Model):
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
        stub = StubExecuteClient(
            [
                {
                    "columns": ["id", "name", "email", "age", "is_active"],
                    "rows": [[42, "Alice", None, 25, True]],
                }
            ]
        )

        instance = OxydeTestModel(name="Alice", age=25)
        result = await instance.save(client=stub)

        assert stub.calls[0]["op"] == "insert"
        assert result is instance
        assert instance.id == 42

    @pytest.mark.asyncio
    async def test_save_existing_instance_updates(self):
        """Test save() on existing instance (with PK) updates record."""
        stub = StubExecuteClient(
            [
                {
                    "columns": ["id", "name", "email", "age", "is_active"],
                    "rows": [[1, "Bob", None, 30, True]],
                }
            ]
        )

        instance = OxydeTestModel(id=1, name="Bob", age=30)
        result = await instance.save(client=stub)

        assert stub.calls[0]["op"] == "update"
        assert result is instance

    @pytest.mark.asyncio
    async def test_save_with_update_fields(self):
        """Test save() with update_fields updates only specified fields."""
        stub = StubExecuteClient(
            [
                {
                    "columns": ["id", "name", "email", "age", "is_active"],
                    "rows": [[1, "Charlie", "c@example.com", 35, True]],
                }
            ]
        )

        instance = OxydeTestModel(id=1, name="Charlie", email="c@example.com", age=35)
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

        instance = OxydeTestModel(id=1, name="Test")

        with pytest.raises(FieldError):
            await instance.save(client=stub, update_fields=["nonexistent"])

    @pytest.mark.asyncio
    async def test_save_update_fields_resolves_fk_name(self):
        """save(update_fields=["author"]) resolves to synthetic column "author_id"."""

        class _Author(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class _Post(Model):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            author: _Author | None = None

            class Meta:
                is_table = True

        stub = StubExecuteClient(
            [
                {
                    "columns": ["id", "title", "author_id"],
                    "rows": [[1, "Hello", 5]],
                }
            ]
        )
        post = _Post(id=1, title="Hello", author_id=5)
        await post.save(client=stub, update_fields=["author"])

        call = stub.calls[0]
        assert call["op"] == "update"
        assert "author_id" in call["values"]
        assert call["values"]["author_id"] == 5

    @pytest.mark.asyncio
    async def test_save_update_fields_rejects_reverse_fk(self):
        """save(update_fields=["posts"]) raises FieldError for reverse FK."""

        class _Author2(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""
            posts: list = Field(db_reverse_fk="author_id")

            class Meta:
                is_table = True

        stub = StubExecuteClient([{"affected": 1}])
        author = _Author2(id=1, name="Alice")

        with pytest.raises(FieldError, match="virtual relation field"):
            await author.save(client=stub, update_fields=["posts"])

    @pytest.mark.asyncio
    async def test_save_update_not_found_raises(self):
        """Test save() raises NotFoundError when record not found."""
        stub = StubExecuteClient(
            [
                {"affected": 0, "columns": [], "rows": []},
            ]
        )

        instance = OxydeTestModel(id=999, name="Ghost")

        with pytest.raises(NotFoundError):
            await instance.save(client=stub)


class TestModelDelete:
    """Test Model.delete() instance method."""

    @pytest.mark.asyncio
    async def test_delete_removes_record(self):
        """Test delete() removes record from database."""
        stub = StubExecuteClient([{"affected": 1}])

        instance = OxydeTestModel(id=1, name="ToDelete")
        affected = await instance.delete(client=stub)

        assert stub.calls[0]["op"] == "delete"
        assert affected == 1

    @pytest.mark.asyncio
    async def test_delete_without_pk_raises(self):
        """Test delete() without PK value raises error."""
        stub = StubExecuteClient([])

        instance = OxydeTestModel(name="NoPK")
        instance.id = None

        with pytest.raises(ManagerError):
            await instance.delete(client=stub)

    @pytest.mark.asyncio
    async def test_delete_returns_affected_count(self):
        """Test delete() returns number of affected rows."""
        stub = StubExecuteClient([{"affected": 0}])

        instance = OxydeTestModel(id=999, name="NotFound")
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

        instance = OxydeTestModel(id=1, name="Original", age=25)
        await instance.refresh(client=stub)

        assert instance.name == "Updated"
        assert instance.age == 99

    @pytest.mark.asyncio
    async def test_refresh_without_pk_raises(self):
        """Test refresh() without PK value raises error."""
        stub = StubExecuteClient([])

        instance = OxydeTestModel(name="NoPK")

        with pytest.raises(ManagerError):
            await instance.refresh(client=stub)


class TestManagerCreate:
    """Test QueryManager.create() method."""

    @pytest.mark.asyncio
    async def test_create_with_kwargs(self):
        """Test create() with keyword arguments."""
        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])

        instance = await OxydeTestModel.objects.create(
            client=stub, name="NewUser", age=25
        )

        assert isinstance(instance, OxydeTestModel)
        assert instance.name == "NewUser"
        assert stub.calls[0]["op"] == "insert"

    @pytest.mark.asyncio
    async def test_create_requires_data(self):
        """Test create() without data raises error."""
        stub = StubExecuteClient([])

        with pytest.raises(ManagerError):
            await OxydeTestModel.objects.create(client=stub)

    @pytest.mark.asyncio
    async def test_create_instance_and_kwargs_raises(self):
        """Test create() with both instance and kwargs raises error."""
        stub = StubExecuteClient([])

        with pytest.raises(ManagerError):
            await OxydeTestModel.objects.create(
                client=stub, instance=OxydeTestModel(name="Test"), name="Other"
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
        created = await OxydeTestModel.objects.bulk_create(objects, client=stub)

        assert len(created) == 2
        assert stub.calls[0]["op"] == "insert"

    @pytest.mark.asyncio
    async def test_bulk_create_with_instances(self):
        """Test bulk_create() with model instances."""
        stub = StubExecuteClient([{"affected": 2, "inserted_ids": [1, 2]}])

        objects = [
            OxydeTestModel(name="User1", age=20),
            OxydeTestModel(name="User2", age=30),
        ]
        created = await OxydeTestModel.objects.bulk_create(objects, client=stub)

        assert len(created) == 2
        assert created[0].id == 1
        assert created[1].id == 2

    @pytest.mark.asyncio
    async def test_bulk_create_mixed(self):
        """Test bulk_create() with mixed dicts and instances."""
        stub = StubExecuteClient([{"affected": 2, "inserted_ids": [1, 2]}])

        objects = [
            {"name": "Dict", "age": 20},
            OxydeTestModel(name="Instance", age=30),
        ]
        created = await OxydeTestModel.objects.bulk_create(objects, client=stub)

        assert len(created) == 2

    @pytest.mark.asyncio
    async def test_bulk_create_empty_list(self):
        """Test bulk_create() with empty list returns empty."""
        stub = StubExecuteClient([])

        created = await OxydeTestModel.objects.bulk_create([], client=stub)

        assert created == []
        assert len(stub.calls) == 0


class TestManagerBulkUpdate:
    """Test QueryManager.bulk_update() method."""

    @pytest.mark.asyncio
    async def test_bulk_update_updates_records(self):
        """Test bulk_update() updates multiple records."""
        stub = StubExecuteClient([{"affected": 2}])

        objects = [
            OxydeTestModel(id=1, name="Updated1", age=25),
            OxydeTestModel(id=2, name="Updated2", age=35),
        ]
        affected = await OxydeTestModel.objects.bulk_update(
            objects, ["name", "age"], client=stub
        )

        assert affected == 2

    @pytest.mark.asyncio
    async def test_bulk_update_requires_fields(self):
        """Test bulk_update() requires at least one field."""
        stub = StubExecuteClient([])

        objects = [OxydeTestModel(id=1, name="Test")]

        with pytest.raises(ManagerError):
            await OxydeTestModel.objects.bulk_update(objects, [], client=stub)

    @pytest.mark.asyncio
    async def test_bulk_update_empty_list(self):
        """Test bulk_update() with empty list returns 0."""
        stub = StubExecuteClient([])

        affected = await OxydeTestModel.objects.bulk_update([], ["name"], client=stub)

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

        obj, created = await OxydeTestModel.objects.get_or_create(
            client=stub, name="Existing"
        )

        assert created is False
        assert isinstance(obj, OxydeTestModel)
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

        obj, created = await OxydeTestModel.objects.get_or_create(
            client=stub, defaults={"age": 30}, name="NewUser"
        )

        assert created is True
        assert isinstance(obj, OxydeTestModel)
        assert obj.name == "NewUser"

    @pytest.mark.asyncio
    async def test_get_or_create_uses_defaults(self):
        """Test get_or_create() uses defaults for new record."""
        stub = StubExecuteClient([[], {"affected": 1, "inserted_ids": [1]}])

        obj, created = await OxydeTestModel.objects.get_or_create(
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

        obj = await OxydeTestModel.objects.get(client=stub, name="Found")

        assert isinstance(obj, OxydeTestModel)
        assert obj.name == "Found"

    @pytest.mark.asyncio
    async def test_get_not_found_raises(self):
        """Test get() raises NotFoundError when no match."""
        stub = StubExecuteClient([[]])

        with pytest.raises(NotFoundError):
            await OxydeTestModel.objects.get(client=stub, name="Missing")

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
            await OxydeTestModel.objects.get(client=stub, name="Dup")


class TestManagerGetOrNone:
    """Test QueryManager.get_or_none() method."""

    @pytest.mark.asyncio
    async def test_get_or_none_finds_record(self):
        """Test get_or_none() finds matching record."""
        stub = StubExecuteClient(
            [[{"id": 1, "name": "Found", "email": None, "age": 25, "is_active": True}]]
        )

        obj = await OxydeTestModel.objects.get_or_none(client=stub, name="Found")

        assert obj is not None
        assert obj.name == "Found"

    @pytest.mark.asyncio
    async def test_get_or_none_returns_none_when_not_found(self):
        """Test get_or_none() returns None when no match."""
        stub = StubExecuteClient([[]])

        obj = await OxydeTestModel.objects.get_or_none(client=stub, name="Missing")

        assert obj is None


class TestManagerDelete:
    """Test QueryManager.delete() method."""

    @pytest.mark.asyncio
    async def test_filter_delete(self):
        """Test delete() through filter."""
        stub = StubExecuteClient([{"affected": 5}])

        affected = await OxydeTestModel.objects.filter(is_active=False).delete(
            client=stub
        )

        assert affected == 5
        assert stub.calls[0]["op"] == "delete"


class TestManagerUpdate:
    """Test QueryManager.update() method."""

    @pytest.mark.asyncio
    async def test_filter_update(self):
        """Test update() through filter."""
        stub = StubExecuteClient([{"affected": 3}])

        # update() takes keyword arguments, not positional dict
        result = await OxydeTestModel.objects.filter(is_active=True).update(
            age=25, client=stub
        )

        assert result == 3
        assert stub.calls[0]["op"] == "update"


class TestManagerFirstLast:
    """Test QueryManager.first() and last() methods."""

    @pytest.mark.asyncio
    async def test_first_returns_first_record(self):
        """Test first() returns first record ordered by PK."""
        stub = StubExecuteClient(
            [[{"id": 1, "name": "First", "email": None, "age": 20, "is_active": True}]]
        )

        obj = await OxydeTestModel.objects.first(client=stub)

        assert obj is not None
        assert obj.id == 1

    @pytest.mark.asyncio
    async def test_first_returns_none_when_empty(self):
        """Test first() returns None when no records."""
        stub = StubExecuteClient([[]])

        obj = await OxydeTestModel.objects.first(client=stub)

        assert obj is None

    @pytest.mark.asyncio
    async def test_last_returns_last_record(self):
        """Test last() returns last record ordered by -PK."""
        stub = StubExecuteClient(
            [[{"id": 99, "name": "Last", "email": None, "age": 50, "is_active": True}]]
        )

        obj = await OxydeTestModel.objects.last(client=stub)

        assert obj is not None
        assert obj.id == 99


class TestManagerCount:
    """Test QueryManager.count() method."""

    @pytest.mark.asyncio
    async def test_count_returns_count(self):
        """Test count() returns record count."""
        # count() expects aggregate result with _count field
        stub = StubExecuteClient([[{"_count": 3}]])

        count = await OxydeTestModel.objects.count(client=stub)

        assert count == 3


class TestComputedFieldExclusion:
    """Computed fields must not appear in INSERT/UPDATE IR payloads."""

    @pytest.mark.asyncio
    async def test_create_excludes_computed_field(self):
        """create() must not send computed field to the database."""

        class _Product(Model):
            id: int | None = Field(default=None, db_pk=True)
            price: float = Field(default=0.0)
            quantity: int = Field(default=1)

            @computed_field
            @property
            def total(self) -> float:
                return self.price * self.quantity

            class Meta:
                is_table = True

        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])

        instance = await _Product.objects.create(client=stub, price=10.0, quantity=3)

        assert instance.total == 30.0
        assert "total" not in stub.calls[0]["values"]

    @pytest.mark.asyncio
    async def test_bulk_create_excludes_computed_field(self):
        """bulk_create() must not send computed field to the database."""

        class _Product2(Model):
            id: int | None = Field(default=None, db_pk=True)
            price: float = Field(default=0.0)
            quantity: int = Field(default=1)

            @computed_field
            @property
            def total(self) -> float:
                return self.price * self.quantity

            class Meta:
                is_table = True

        stub = StubExecuteClient([{"affected": 2, "inserted_ids": [1, 2]}])

        objects = [
            _Product2(price=5.0, quantity=2),
            _Product2(price=20.0, quantity=4),
        ]
        created = await _Product2.objects.bulk_create(objects, client=stub)

        assert len(created) == 2
        assert created[0].total == 10.0
        for call in stub.calls:
            for row in call.get("bulk_values", []):
                assert "total" not in row


class TestManagerUpdateOrCreate:
    """Test QueryManager.update_or_create() method."""

    @pytest.mark.asyncio
    async def test_update_or_create_updates_existing_via_save(self):
        """update_or_create() updates existing objects through the normal save path."""
        stub = StubExecuteClient(
            [
                [
                    {
                        "id": 1,
                        "name": "Existing",
                        "email": "existing@example.com",
                        "age": 25,
                        "is_active": True,
                    }
                ],
                {
                    "columns": ["id", "name", "email", "age", "is_active"],
                    "rows": [[1, "Existing", "existing@example.com", 25, True]],
                },
            ]
        )

        obj, created = await OxydeTestModel.objects.update_or_create(
            client=stub,
            defaults={"age": 25},
            id=1,
        )

        assert created is False
        assert isinstance(obj, OxydeTestModel)
        assert obj.id == 1
        assert obj.age == 25
        assert [call["op"] for call in stub.calls] == ["select", "update"]
        assert stub.calls[1]["values"] == {"age": 25}

    @pytest.mark.asyncio
    async def test_update_or_create_retries_after_integrity_error(self):
        """update_or_create() retries with get() after a conflicting create()."""

        class IntegrityRetryStub(StubExecuteClient):
            async def execute(self, ir):
                if ir["op"] == "insert":
                    self.calls.append(ir)
                    raise IntegrityError("duplicate key value violates unique constraint")
                return await super().execute(ir)

        stub = IntegrityRetryStub(
            [
                [],
                [
                    {
                        "id": 1,
                        "name": "Recovered",
                        "email": "existing@example.com",
                        "age": 25,
                        "is_active": True,
                    }
                ],
                {
                    "columns": ["id", "name", "email", "age", "is_active"],
                    "rows": [[1, "Recovered", "existing@example.com", 30, True]],
                },
            ]
        )

        obj, created = await OxydeTestModel.objects.update_or_create(
            client=stub,
            defaults={"name": "Recovered", "age": 30},
            id=1,
        )

        assert created is False
        assert isinstance(obj, OxydeTestModel)
        assert obj.id == 1
        assert obj.name == "Recovered"
        assert obj.age == 30
        assert [call["op"] for call in stub.calls] == ["select", "insert", "select", "update"]
        assert stub.calls[3]["values"] == {"name": "Recovered", "age": 30}

    @pytest.mark.asyncio
    async def test_update_or_create_existing_without_defaults_skips_update(self):
        """update_or_create() returns existing records unchanged when defaults are omitted."""
        stub = StubExecuteClient(
            [
                [
                    {
                        "id": 1,
                        "name": "Existing",
                        "email": "existing@example.com",
                        "age": 25,
                        "is_active": True,
                    }
                ]
            ]
        )

        obj, created = await OxydeTestModel.objects.update_or_create(
            client=stub,
            id=1,
        )

        assert created is False
        assert obj.id == 1
        assert len(stub.calls) == 1

    @pytest.mark.asyncio
    async def test_update_or_create_creates_new(self):
        """update_or_create() creates new record when not found."""
        stub = StubExecuteClient(
            [
                [],  # First query returns empty
                {"affected": 1, "inserted_ids": [42]},  # Then insert
            ]
        )

        obj, created = await OxydeTestModel.objects.update_or_create(
            client=stub,
            defaults={"name": "NewUser", "age": 30},
            id=42,
        )

        assert created is True
        assert isinstance(obj, OxydeTestModel)
        assert obj.id == 42
        assert obj.name == "NewUser"
        assert stub.calls[1]["op"] == "insert"
        assert "on_conflict" not in stub.calls[1]

    @pytest.mark.asyncio
    async def test_update_or_create_updates_non_unique_lookup(self):
        """update_or_create() uses the same safe path for non-unique lookups."""
        stub = StubExecuteClient(
            [
                [
                    {
                        "id": 1,
                        "name": "Existing",
                        "email": "existing@example.com",
                        "age": 25,
                        "is_active": True,
                    }
                ],
                {
                    "columns": ["id", "name", "email", "age", "is_active"],
                    "rows": [[1, "Existing", "existing@example.com", 30, True]],
                },
            ]
        )

        obj, created = await OxydeTestModel.objects.update_or_create(
            client=stub,
            defaults={"age": 30},
            name="Existing",
        )

        assert created is False
        assert obj.age == 30
        assert [call["op"] for call in stub.calls] == [
            "select",
            "update",
        ]

    @pytest.mark.asyncio
    async def test_update_or_create_uses_aliased_column_names(self):
        """update_or_create() keeps aliased fields on the standard save/update path."""

        class AliasedModel(Model):
            id: int | None = Field(default=None, db_pk=True)
            title: str = Field(db_column="event_title")

            class Meta:
                is_table = True

        stub = StubExecuteClient(
            [
                [{"id": 1, "title": "Original"}],
                {"columns": ["id", "event_title"], "rows": [[1, "Updated"]]},
            ]
        )

        obj, created = await AliasedModel.objects.update_or_create(
            client=stub,
            defaults={"title": "Updated"},
            id=1,
        )

        assert created is False
        assert obj.title == "Updated"
        assert [call["op"] for call in stub.calls] == ["select", "update"]
        assert stub.calls[1]["values"] == {"event_title": "Updated"}


class TestManagerUpsert:
    """Test QueryManager.upsert() placeholder."""

    @pytest.mark.asyncio
    async def test_upsert_raises_not_implemented(self):
        """Test upsert() raises ManagerError (not implemented)."""
        with pytest.raises(ManagerError):
            await OxydeTestModel.objects.upsert()

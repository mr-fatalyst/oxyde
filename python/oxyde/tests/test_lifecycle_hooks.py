"""Tests for OxydeModel lifecycle hooks: pre_save, post_save, pre_delete, post_delete."""

from __future__ import annotations

from typing import Any

import msgpack
import pytest

from oxyde import Field, OxydeModel
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


class TestPreSaveHook:
    """Test pre_save lifecycle hook."""

    @pytest.mark.asyncio
    async def test_pre_save_called_on_create(self):
        """Test pre_save is called when creating a new instance."""
        hook_calls: list[dict[str, Any]] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append({
                    "is_create": is_create,
                    "update_fields": update_fields,
                    "name": self.name,
                })

        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])
        instance = HookedModel(name="Alice")
        await instance.save(client=stub)

        assert len(hook_calls) == 1
        assert hook_calls[0]["is_create"] is True
        assert hook_calls[0]["update_fields"] is None
        assert hook_calls[0]["name"] == "Alice"

    @pytest.mark.asyncio
    async def test_pre_save_called_on_update(self):
        """Test pre_save is called when updating an existing instance."""
        hook_calls: list[dict[str, Any]] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append({
                    "is_create": is_create,
                    "update_fields": update_fields,
                })

        stub = StubExecuteClient([{
            "columns": ["id", "name"],
            "rows": [[1, "Bob"]]
        }])
        instance = HookedModel(id=1, name="Bob")
        await instance.save(client=stub)

        assert len(hook_calls) == 1
        assert hook_calls[0]["is_create"] is False
        assert hook_calls[0]["update_fields"] is None

    @pytest.mark.asyncio
    async def test_pre_save_receives_update_fields(self):
        """Test pre_save receives update_fields when specified."""
        hook_calls: list[dict[str, Any]] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            email: str | None = None

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append({
                    "is_create": is_create,
                    "update_fields": update_fields,
                })

        stub = StubExecuteClient([{
            "columns": ["id", "name", "email"],
            "rows": [[1, "Charlie", "c@example.com"]]
        }])
        instance = HookedModel(id=1, name="Charlie", email="c@example.com")
        await instance.save(client=stub, update_fields=["name"])

        assert len(hook_calls) == 1
        assert hook_calls[0]["update_fields"] == {"name"}

    @pytest.mark.asyncio
    async def test_pre_save_can_modify_instance(self):
        """Test pre_save can modify instance fields before save."""

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            slug: str | None = None

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                # Auto-generate slug from name
                self.slug = self.name.lower().replace(" ", "-")

        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])
        instance = HookedModel(name="Hello World")
        await instance.save(client=stub)

        assert instance.slug == "hello-world"
        # Check that slug was included in the insert
        assert "slug" in stub.calls[0]["values"]
        assert stub.calls[0]["values"]["slug"] == "hello-world"


class TestPostSaveHook:
    """Test post_save lifecycle hook."""

    @pytest.mark.asyncio
    async def test_post_save_called_after_create(self):
        """Test post_save is called after creating a new instance."""
        hook_calls: list[dict[str, Any]] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def post_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append({
                    "is_create": is_create,
                    "update_fields": update_fields,
                    "id": self.id,  # Should have ID assigned
                })

        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [42]}])
        instance = HookedModel(name="Alice")
        await instance.save(client=stub)

        assert len(hook_calls) == 1
        assert hook_calls[0]["is_create"] is True
        assert hook_calls[0]["id"] == 42  # ID was assigned before post_save

    @pytest.mark.asyncio
    async def test_post_save_called_after_update(self):
        """Test post_save is called after updating an existing instance."""
        hook_calls: list[dict[str, Any]] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def post_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append({
                    "is_create": is_create,
                })

        stub = StubExecuteClient([{
            "columns": ["id", "name"],
            "rows": [[1, "Bob"]]
        }])
        instance = HookedModel(id=1, name="Bob")
        await instance.save(client=stub)

        assert len(hook_calls) == 1
        assert hook_calls[0]["is_create"] is False

    @pytest.mark.asyncio
    async def test_hooks_called_in_order(self):
        """Test pre_save is called before post_save."""
        call_order: list[str] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                call_order.append("pre_save")

            async def post_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                call_order.append("post_save")

        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])
        instance = HookedModel(name="Test")
        await instance.save(client=stub)

        assert call_order == ["pre_save", "post_save"]


class TestPreDeleteHook:
    """Test pre_delete lifecycle hook."""

    @pytest.mark.asyncio
    async def test_pre_delete_called_before_delete(self):
        """Test pre_delete is called before deleting an instance."""
        hook_calls: list[int] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_delete(self) -> None:
                hook_calls.append(self.id)  # type: ignore

        stub = StubExecuteClient([{"affected": 1}])
        instance = HookedModel(id=42, name="ToDelete")
        await instance.delete(client=stub)

        assert len(hook_calls) == 1
        assert hook_calls[0] == 42


class TestPostDeleteHook:
    """Test post_delete lifecycle hook."""

    @pytest.mark.asyncio
    async def test_post_delete_called_after_delete(self):
        """Test post_delete is called after deleting an instance."""
        hook_calls: list[int] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def post_delete(self) -> None:
                hook_calls.append(self.id)  # type: ignore

        stub = StubExecuteClient([{"affected": 1}])
        instance = HookedModel(id=42, name="ToDelete")
        await instance.delete(client=stub)

        assert len(hook_calls) == 1
        assert hook_calls[0] == 42

    @pytest.mark.asyncio
    async def test_delete_hooks_called_in_order(self):
        """Test pre_delete is called before post_delete."""
        call_order: list[str] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_delete(self) -> None:
                call_order.append("pre_delete")

            async def post_delete(self) -> None:
                call_order.append("post_delete")

        stub = StubExecuteClient([{"affected": 1}])
        instance = HookedModel(id=1, name="Test")
        await instance.delete(client=stub)

        assert call_order == ["pre_delete", "post_delete"]


class TestManagerCreateHooks:
    """Test hooks work with QueryManager.create()."""

    @pytest.mark.asyncio
    async def test_create_calls_hooks(self):
        """Test QueryManager.create() calls pre_save and post_save."""
        hook_calls: list[str] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append(f"pre_save:is_create={is_create}")

            async def post_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append(f"post_save:is_create={is_create}")

        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])
        await HookedModel.objects.create(name="Test", client=stub)

        assert hook_calls == [
            "pre_save:is_create=True",
            "post_save:is_create=True",
        ]


class TestGetOrCreateHooks:
    """Test hooks work with QueryManager.get_or_create()."""

    @pytest.mark.asyncio
    async def test_get_or_create_calls_hooks_on_create(self):
        """Test get_or_create() calls hooks when creating."""
        hook_calls: list[str] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append("pre_save")

            async def post_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append("post_save")

        # First call returns empty (not found), second is the create
        stub = StubExecuteClient([
            [],  # get() returns empty
            {"affected": 1, "inserted_ids": [1]},  # create()
        ])
        obj, created = await HookedModel.objects.get_or_create(name="Test", client=stub)

        assert created is True
        assert hook_calls == ["pre_save", "post_save"]

    @pytest.mark.asyncio
    async def test_get_or_create_no_hooks_on_get(self):
        """Test get_or_create() does not call hooks when getting existing."""
        hook_calls: list[str] = []

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append("pre_save")

            async def post_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                hook_calls.append("post_save")

        stub = StubExecuteClient([
            [{"id": 1, "name": "Existing"}],  # get() returns existing
        ])
        obj, created = await HookedModel.objects.get_or_create(name="Existing", client=stub)

        assert created is False
        assert hook_calls == []  # No hooks called


class TestHookExceptionHandling:
    """Test exception handling in hooks."""

    @pytest.mark.asyncio
    async def test_pre_save_exception_prevents_save(self):
        """Test exception in pre_save prevents the save operation."""

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                raise ValueError("Validation failed")

        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])
        instance = HookedModel(name="Test")

        with pytest.raises(ValueError, match="Validation failed"):
            await instance.save(client=stub)

        # No database call should have been made
        assert len(stub.calls) == 0

    @pytest.mark.asyncio
    async def test_post_save_exception_propagates(self):
        """Test exception in post_save propagates after save completes."""

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def post_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                raise RuntimeError("Post-save failed")

        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])
        instance = HookedModel(name="Test")

        with pytest.raises(RuntimeError, match="Post-save failed"):
            await instance.save(client=stub)

        # Database call was made before exception
        assert len(stub.calls) == 1
        assert instance.id == 1  # ID was assigned

    @pytest.mark.asyncio
    async def test_pre_delete_exception_prevents_delete(self):
        """Test exception in pre_delete prevents the delete operation."""

        class HookedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_delete(self) -> None:
                raise ValueError("Cannot delete")

        stub = StubExecuteClient([{"affected": 1}])
        instance = HookedModel(id=1, name="Test")

        with pytest.raises(ValueError, match="Cannot delete"):
            await instance.delete(client=stub)

        # No database call should have been made
        assert len(stub.calls) == 0


class TestHookInheritance:
    """Test hook inheritance behavior."""

    @pytest.mark.asyncio
    async def test_hooks_can_call_super(self):
        """Test child hooks can call super() to chain behavior."""
        call_order: list[str] = []

        class BaseModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                call_order.append("base_pre_save")

        class ChildModel(BaseModel):
            class Meta:
                is_table = True

            async def pre_save(
                self, *, is_create: bool, update_fields: set[str] | None = None
            ) -> None:
                call_order.append("child_pre_save_before")
                await super().pre_save(is_create=is_create, update_fields=update_fields)
                call_order.append("child_pre_save_after")

        stub = StubExecuteClient([{"affected": 1, "inserted_ids": [1]}])
        instance = ChildModel(name="Test")
        await instance.save(client=stub)

        assert call_order == [
            "child_pre_save_before",
            "base_pre_save",
            "child_pre_save_after",
        ]

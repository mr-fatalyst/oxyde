"""Integration tests for CRUD operations."""
from __future__ import annotations

import pytest

from oxyde import F
from oxyde.exceptions import MultipleObjectsReturned, NotFoundError
from oxyde.queries import execute_raw

from .conftest import Author, Post, create_author, create_post


class TestCreate:
    @pytest.mark.asyncio
    async def test_create_single(self, db):
        author = await Author.objects.create(
            name="Diana", email="diana@test.com", client=db
        )
        assert author.name == "Diana"
        assert author.email == "diana@test.com"
        assert author.id is not None

    @pytest.mark.asyncio
    async def test_create_with_defaults(self, db):
        """Fields with defaults should get default values when not passed."""
        post = await Post.objects.create(
            title="Default Test", author_id=1, client=db
        )
        assert post.title == "Default Test"
        assert post.body == ""
        assert post.views == 0
        assert post.published == False  # noqa: E712 — SQLite returns int

    @pytest.mark.asyncio
    async def test_create_with_null_fk(self, db):
        """Nullable FK can be None."""
        post = await Post.objects.create(
            title="No Category", author_id=1, client=db
        )
        assert post.category_id is None


class TestBulkCreate:
    @pytest.mark.asyncio
    async def test_bulk_create(self, db):
        authors = [
            Author(name="X", email="x@test.com"),
            Author(name="Y", email="y@test.com"),
        ]
        created = await Author.objects.bulk_create(authors, client=db)
        assert len(created) == 2
        assert created[0].name == "X"
        assert created[1].name == "Y"

        count = await Author.objects.count(client=db)
        assert count == 5  # 3 seed + 2 new

    @pytest.mark.asyncio
    async def test_bulk_create_empty(self, db):
        created = await Author.objects.bulk_create([], client=db)
        assert created == []


class TestGet:
    @pytest.mark.asyncio
    async def test_get_found(self, db):
        author = await Author.objects.get(id=1, client=db)
        assert author.name == "Alice"
        assert author.email == "alice@test.com"

    @pytest.mark.asyncio
    async def test_get_not_found(self, db):
        with pytest.raises(NotFoundError):
            await Author.objects.get(id=999, client=db)

    @pytest.mark.asyncio
    async def test_get_multiple(self, db):
        with pytest.raises(MultipleObjectsReturned):
            await Author.objects.get(active=True, client=db)

    @pytest.mark.asyncio
    async def test_get_or_none_found(self, db):
        author = await Author.objects.get_or_none(id=1, client=db)
        assert author is not None
        assert author.name == "Alice"

    @pytest.mark.asyncio
    async def test_get_or_none_not_found(self, db):
        author = await Author.objects.get_or_none(id=999, client=db)
        assert author is None

    @pytest.mark.asyncio
    async def test_get_or_create_existing(self, db):
        author, created = await Author.objects.get_or_create(
            email="alice@test.com",
            defaults={"name": "Alice Clone"},
            client=db,
        )
        assert created is False
        assert author.name == "Alice"

    @pytest.mark.asyncio
    async def test_get_or_create_new(self, db):
        author, created = await Author.objects.get_or_create(
            email="new@test.com",
            defaults={"name": "New Author"},
            client=db,
        )
        assert created is True
        assert author.name == "New Author"
        assert author.email == "new@test.com"


class TestSave:
    @pytest.mark.asyncio
    async def test_save_insert(self, db):
        author = Author(name="SavedNew", email="saved@test.com")
        saved = await author.save(client=db)
        assert saved.id is not None

        fetched = await Author.objects.get(id=saved.id, client=db)
        assert fetched.name == "SavedNew"

    @pytest.mark.asyncio
    async def test_save_update(self, db):
        author = await Author.objects.get(id=1, client=db)
        author.name = "Alice Updated"
        await author.save(client=db)

        refreshed = await Author.objects.get(id=1, client=db)
        assert refreshed.name == "Alice Updated"


class TestDelete:
    @pytest.mark.asyncio
    async def test_delete_instance(self, db):
        # Create an author with no FK dependencies so delete succeeds
        author = await create_author(db, name="ToDelete")
        author_id = author.id

        result = await author.delete(client=db)
        assert result >= 1

        deleted = await Author.objects.get_or_none(id=author_id, client=db)
        assert deleted is None

    @pytest.mark.asyncio
    async def test_delete_queryset(self, db):
        deleted = await Post.objects.filter(published=False).delete(client=db)
        assert deleted == 2  # Draft Post + Unpublished

        remaining = await Post.objects.count(client=db)
        assert remaining == 4


class TestRefresh:
    @pytest.mark.asyncio
    async def test_refresh(self, db):
        author = await Author.objects.get(id=1, client=db)
        assert author.name == "Alice"

        # Modify directly in DB
        await execute_raw(
            "UPDATE authors SET name = ? WHERE id = ?",
            ["Alice Modified", 1],
            client=db,
        )

        # Instance still has old value
        assert author.name == "Alice"

        # Refresh reloads from DB
        await author.refresh(client=db)
        assert author.name == "Alice Modified"


class TestUpdate:
    @pytest.mark.asyncio
    async def test_update_queryset(self, db):
        result = await Post.objects.filter(published=True).update(
            views=0, client=db
        )
        assert len(result) == 4  # 4 published posts

        post = await Post.objects.get(id=1, client=db)
        assert post.views == 0

    @pytest.mark.asyncio
    async def test_update_with_f_expression(self, db):
        await Post.objects.filter(id=1).update(
            views=F("views") + 1, client=db
        )
        post = await Post.objects.get(id=1, client=db)
        assert post.views == 121  # 120 + 1

    @pytest.mark.asyncio
    async def test_increment(self, db):
        result = await Post.objects.filter(id=1).increment(
            "views", by=5, client=db
        )
        assert result >= 1

        post = await Post.objects.get(id=1, client=db)
        assert post.views == 125  # 120 + 5

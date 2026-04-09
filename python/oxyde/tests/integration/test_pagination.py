"""Integration tests for pagination, ordering, values, and queryset utilities."""
from __future__ import annotations

import pytest

from .conftest import Author, Post


class TestLimitOffset:
    @pytest.mark.asyncio
    async def test_limit(self, db):
        posts = await Post.objects.order_by("id").limit(2).all(using=db.name)
        assert len(posts) == 2
        assert posts[0].id == 1
        assert posts[1].id == 2

    @pytest.mark.asyncio
    async def test_offset(self, db):
        # SQLite requires LIMIT with OFFSET, use large limit
        posts = await Post.objects.order_by("id").limit(100).offset(4).all(using=db.name)
        assert len(posts) == 2  # posts 5 and 6

    @pytest.mark.asyncio
    async def test_limit_offset(self, db):
        posts = await Post.objects.order_by("id").limit(2).offset(1).all(using=db.name)
        assert len(posts) == 2
        assert posts[0].id == 2
        assert posts[1].id == 3

    @pytest.mark.asyncio
    async def test_slicing(self, db):
        posts = await Post.objects.order_by("id")[1:3].all(using=db.name)
        assert len(posts) == 2
        assert posts[0].id == 2
        assert posts[1].id == 3


class TestOrderBy:
    @pytest.mark.asyncio
    async def test_order_by_asc(self, db):
        authors = await Author.objects.order_by("name").all(using=db.name)
        names = [a.name for a in authors]
        assert names == ["Alice", "Bob", "Charlie"]

    @pytest.mark.asyncio
    async def test_order_by_desc(self, db):
        authors = await Author.objects.order_by("-name").all(using=db.name)
        names = [a.name for a in authors]
        assert names == ["Charlie", "Bob", "Alice"]

    @pytest.mark.asyncio
    async def test_order_by_multiple(self, db):
        """Order by active (asc), then name (desc)."""
        authors = await Author.objects.order_by("active", "-name").all(using=db.name)
        names = [a.name for a in authors]
        # active=0: Charlie; active=1: Bob, Alice (desc)
        assert names == ["Charlie", "Bob", "Alice"]


class TestOrderByRandom:
    @pytest.mark.asyncio
    async def test_order_by_random_returns_all_records(self, db):
        authors = await Author.objects.order_by("?").all(using=db.name)
        assert len(authors) == 3
        assert {a.name for a in authors} == {"Alice", "Bob", "Charlie"}

    @pytest.mark.asyncio
    async def test_order_by_random_combined_with_limit(self, db):
        authors = await Author.objects.order_by("?").limit(2).all(using=db.name)
        assert len(authors) == 2


class TestDistinct:
    @pytest.mark.asyncio
    async def test_distinct(self, db):
        author_ids = await (
            Post.objects.values_list("author_id", flat=True).distinct().all(using=db.name)
        )
        assert sorted(author_ids) == [1, 2, 3]


class TestValues:
    @pytest.mark.asyncio
    async def test_values(self, db):
        rows = await Author.objects.order_by("id").values("id", "name").all(using=db.name)
        assert len(rows) == 3
        assert rows[0] == {"id": 1, "name": "Alice"}

    @pytest.mark.asyncio
    async def test_values_list(self, db):
        rows = await Author.objects.order_by("id").values_list("id", "name").all(
            using=db.name
        )
        assert len(rows) == 3
        assert rows[0] == (1, "Alice")

    @pytest.mark.asyncio
    async def test_values_list_flat(self, db):
        names = await Author.objects.order_by("name").values_list(
            "name", flat=True
        ).all(using=db.name)
        assert names == ["Alice", "Bob", "Charlie"]


class TestCountExists:
    @pytest.mark.asyncio
    async def test_count(self, db):
        count = await Post.objects.count(using=db.name)
        assert count == 6

    @pytest.mark.asyncio
    async def test_exists_true(self, db):
        exists = await Post.objects.filter(published=True).exists(using=db.name)
        assert exists is True

    @pytest.mark.asyncio
    async def test_exists_false(self, db):
        exists = await Post.objects.filter(title="Nonexistent").exists(using=db.name)
        assert exists is False


class TestFirstLast:
    @pytest.mark.asyncio
    async def test_first(self, db):
        post = await Post.objects.order_by("id").first(using=db.name)
        assert post is not None
        assert post.id == 1

    @pytest.mark.asyncio
    async def test_last(self, db):
        post = await Post.objects.order_by("id").last(using=db.name)
        assert post is not None
        assert post.id == 6

"""Integration tests for joins and prefetch (FK, reverse FK, M2M)."""
from __future__ import annotations

import pytest

from .conftest import Author, Post


class TestJoin:
    @pytest.mark.asyncio
    async def test_join_fk(self, db):
        """Join Post → Author via FK."""
        posts = await Post.objects.join("author").order_by("id").all(client=db)
        assert len(posts) == 6

        assert posts[0].author is not None
        assert posts[0].author.name == "Alice"
        assert posts[2].author.name == "Bob"



class TestPrefetch:
    @pytest.mark.asyncio
    async def test_prefetch_one_to_many(self, db):
        """Prefetch Author → Posts (one-to-many)."""
        authors = await (
            Author.objects.prefetch("posts").order_by("id").all(client=db)
        )
        assert len(authors) == 3

        alice = authors[0]
        assert alice.name == "Alice"
        assert len(alice.posts) == 2
        assert {p.title for p in alice.posts} == {"Rust Patterns", "Async Python"}

        charlie = authors[2]
        assert len(charlie.posts) == 2

    @pytest.mark.asyncio
    async def test_prefetch_with_filter(self, db):
        """Prefetch comments only for filtered posts."""
        posts = await (
            Post.objects.filter(id=1).prefetch("comments").all(client=db)
        )
        assert len(posts) == 1
        assert len(posts[0].comments) == 2
        bodies = {c.body for c in posts[0].comments}
        assert bodies == {"Great read!", "Thanks for sharing"}

    @pytest.mark.asyncio
    async def test_prefetch_m2m(self, db):
        """Prefetch Post → Tags via M2M through PostTag."""
        posts = await (
            Post.objects.filter(id__in=[1, 2, 3])
            .prefetch("tags")
            .order_by("id")
            .all(client=db)
        )
        assert len(posts) == 3

        rust_patterns = posts[0]
        tag_names = {t.name for t in rust_patterns.tags}
        assert tag_names == {"Python", "Rust"}

        async_python = posts[1]
        assert {t.name for t in async_python.tags} == {"Python"}

        quantum = posts[2]
        assert {t.name for t in quantum.tags} == {"SQL"}

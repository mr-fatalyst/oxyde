"""Integration tests for filtering and Q expressions."""
from __future__ import annotations

import pytest

from oxyde import Q

from .conftest import Author, Post


class TestBasicFilters:
    @pytest.mark.asyncio
    async def test_filter_exact(self, db):
        authors = await Author.objects.filter(name="Alice").all(client=db)
        assert len(authors) == 1
        assert authors[0].email == "alice@test.com"

    @pytest.mark.asyncio
    async def test_filter_iexact(self, db):
        authors = await Author.objects.filter(name__iexact="alice").all(client=db)
        assert len(authors) == 1
        assert authors[0].name == "Alice"

    @pytest.mark.asyncio
    async def test_filter_contains(self, db):
        posts = await Post.objects.filter(title__contains="Python").all(client=db)
        assert len(posts) == 1
        assert posts[0].title == "Async Python"

    @pytest.mark.asyncio
    async def test_filter_icontains(self, db):
        posts = await Post.objects.filter(title__icontains="python").all(client=db)
        assert len(posts) == 1
        assert posts[0].title == "Async Python"


class TestNumericFilters:
    @pytest.mark.asyncio
    async def test_filter_gt(self, db):
        posts = await Post.objects.filter(views__gt=100).all(client=db)
        assert len(posts) == 2  # Rust Patterns (120), ML Basics (200)

    @pytest.mark.asyncio
    async def test_filter_gte(self, db):
        posts = await Post.objects.filter(views__gte=120).all(client=db)
        assert len(posts) == 2  # Rust Patterns (120), ML Basics (200)

    @pytest.mark.asyncio
    async def test_filter_lt(self, db):
        posts = await Post.objects.filter(views__lt=35).all(client=db)
        # views=0 (Draft Post, Unpublished)
        assert len(posts) == 2

    @pytest.mark.asyncio
    async def test_filter_lte(self, db):
        posts = await Post.objects.filter(views__lte=35).all(client=db)
        # views=0 (x2), views=35 (Async Python)
        assert len(posts) == 3

    @pytest.mark.asyncio
    async def test_filter_in(self, db):
        posts = await Post.objects.filter(id__in=[1, 3, 5]).all(client=db)
        assert len(posts) == 3
        titles = {p.title for p in posts}
        assert titles == {"Rust Patterns", "Quantum Computing", "ML Basics"}

    @pytest.mark.asyncio
    async def test_filter_isnull_true(self, db):
        posts = await Post.objects.filter(category_id__isnull=True).all(client=db)
        assert len(posts) == 2  # Draft Post, Unpublished

    @pytest.mark.asyncio
    async def test_filter_isnull_false(self, db):
        posts = await Post.objects.filter(category_id__isnull=False).all(client=db)
        assert len(posts) == 4

    @pytest.mark.asyncio
    async def test_filter_between(self, db):
        posts = await Post.objects.filter(views__between=(35, 120)).all(client=db)
        titles = {p.title for p in posts}
        assert titles == {"Async Python", "Quantum Computing", "Rust Patterns"}


class TestExclude:
    @pytest.mark.asyncio
    async def test_exclude(self, db):
        authors = await Author.objects.exclude(active=False).all(client=db)
        assert len(authors) == 2
        names = {a.name for a in authors}
        assert names == {"Alice", "Bob"}


class TestQExpressions:
    @pytest.mark.asyncio
    async def test_q_and(self, db):
        posts = await Post.objects.filter(
            Q(published=True) & Q(views__gte=100)
        ).all(client=db)
        assert len(posts) == 2  # Rust Patterns (120), ML Basics (200)

    @pytest.mark.asyncio
    async def test_q_or(self, db):
        posts = await Post.objects.filter(
            Q(views__gte=200) | Q(views=0)
        ).all(client=db)
        assert len(posts) == 3  # ML Basics, Draft Post, Unpublished

    @pytest.mark.asyncio
    async def test_q_not(self, db):
        authors = await Author.objects.filter(~Q(active=False)).all(client=db)
        assert len(authors) == 2
        names = {a.name for a in authors}
        assert names == {"Alice", "Bob"}

    @pytest.mark.asyncio
    async def test_q_nested(self, db):
        """(published OR views > 100) AND NOT author_id=3"""
        posts = await Post.objects.filter(
            (Q(published=True) | Q(views__gt=100)) & ~Q(author_id=3)
        ).all(client=db)
        titles = {p.title for p in posts}
        assert titles == {"Rust Patterns", "Async Python", "Quantum Computing"}


class TestFKTraversal:
    @pytest.mark.asyncio
    async def test_filter_fk_traversal(self, db):
        """Filter posts by author's name via FK join."""
        posts = await Post.objects.filter(author__name="Alice").all(client=db)
        assert len(posts) == 2
        titles = {p.title for p in posts}
        assert titles == {"Rust Patterns", "Async Python"}


class TestChaining:
    @pytest.mark.asyncio
    async def test_filter_chaining(self, db):
        """Chained .filter() calls should AND conditions."""
        posts_chained = await (
            Post.objects.filter(published=True).filter(views__gte=100).all(client=db)
        )
        posts_single = await Post.objects.filter(
            published=True, views__gte=100
        ).all(client=db)
        assert len(posts_chained) == len(posts_single)
        assert {p.id for p in posts_chained} == {p.id for p in posts_single}

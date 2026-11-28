"""Integration tests for joins and prefetch (FK, reverse FK, M2M)."""
from __future__ import annotations

import pytest

from oxyde.exceptions import ManagerError, RelationNotLoadedError

from .conftest import (
    Author,
    Category,
    Post,
    RenamedChild,
    RenamedParent,
    create_author,
    create_post,
)


class TestJoin:
    @pytest.mark.asyncio
    async def test_join_fk(self, db):
        """Join Post → Author via FK."""
        posts = await Post.objects.join("author").order_by("id").all(using=db.name)
        assert len(posts) == 6

        assert posts[0].author is not None
        assert posts[0].author.name == "Alice"
        assert posts[2].author.name == "Bob"

    @pytest.mark.asyncio
    async def test_related_model_with_db_column_renames(self, db):
        """Joined rows hydrate a related model whose pk and list field set
        db_column (result columns are aliased by field name)."""
        rust = await RenamedParent.objects.create(
            name="rust", labels=["a", "b"], using=db.name
        )
        go = await RenamedParent.objects.create(name="go", labels=None, using=db.name)
        await RenamedChild.objects.create(name="r1", parent_id=rust.id, using=db.name)
        await RenamedChild.objects.create(name="r2", parent_id=rust.id, using=db.name)
        await RenamedChild.objects.create(name="g1", parent_id=go.id, using=db.name)

        children = await (
            RenamedChild.objects.join("parent").order_by("id").all(using=db.name)
        )
        assert [(c.name, c.parent.id, c.parent.labels) for c in children] == [
            ("r1", rust.id, ["a", "b"]),
            ("r2", rust.id, ["a", "b"]),
            ("g1", go.id, None),
        ]

        # Filtering through the FK joins the parent the same way.
        children = await (
            RenamedChild.objects.filter(parent__name="rust")
            .order_by("id")
            .all(using=db.name)
        )
        assert [c.name for c in children] == ["r1", "r2"]
        assert children[0].parent.labels == ["a", "b"]


class TestPrefetch:
    @pytest.mark.asyncio
    async def test_prefetch_one_to_many(self, db):
        """Prefetch Author → Posts (one-to-many)."""
        authors = await (
            Author.objects.prefetch("posts").order_by("id").all(using=db.name)
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
            Post.objects.filter(id=1).prefetch("comments").all(using=db.name)
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
            .all(using=db.name)
        )
        assert len(posts) == 3

        rust_patterns = posts[0]
        tag_names = {t.name for t in rust_patterns.tags}
        assert tag_names == {"Python", "Rust"}

        async_python = posts[1]
        assert {t.name for t in async_python.tags} == {"Python"}

        quantum = posts[2]
        assert {t.name for t in quantum.tags} == {"SQL"}


class TestRelationLoading:
    """A relation is read only after it was loaded."""

    @pytest.mark.asyncio
    async def test_foreign_key_needs_a_join(self, db):
        author = await create_author(db)
        created = await create_post(db, author_id=author.id)

        post = await Post.objects.get(id=created.id, using=db.name)
        assert post.author_id == author.id
        with pytest.raises(RelationNotLoadedError, match="join"):
            post.author
        # A foreign key without a value has nothing to load.
        assert post.category is None

        query = Post.objects.filter(id=created.id).join("author")
        joined = await query.get(using=db.name)
        assert joined.author.id == author.id

    @pytest.mark.asyncio
    async def test_reverse_and_m2m_need_a_prefetch(self, db):
        author = await create_author(db)
        created = await create_post(db, author_id=author.id)

        plain = await Author.objects.get(id=author.id, using=db.name)
        with pytest.raises(RelationNotLoadedError, match="prefetch"):
            plain.posts
        post = await Post.objects.get(id=created.id, using=db.name)
        with pytest.raises(RelationNotLoadedError, match="prefetch"):
            post.tags

        query = Author.objects.filter(id=author.id).prefetch("posts")
        loaded = await query.get(using=db.name)
        assert [p.id for p in loaded.posts] == [created.id]
        # Loaded and empty is an empty list, not an error.
        query = Post.objects.filter(id=created.id).prefetch("tags", "comments")
        post = await query.get(using=db.name)
        assert post.tags == []
        assert post.comments == []

    @pytest.mark.asyncio
    async def test_refresh_drops_a_relation_that_moved(self, db):
        first = await create_author(db)
        second = await create_author(db)
        created = await create_post(db, author_id=first.id)

        query = Post.objects.filter(id=created.id).join("author")
        post = await query.get(using=db.name)
        assert post.author.id == first.id
        await Post.objects.filter(id=created.id).update(
            author_id=second.id, using=db.name
        )
        await post.refresh(using=db.name)
        assert post.author_id == second.id
        with pytest.raises(RelationNotLoadedError):
            post.author


class TestForeignKeyWrites:
    """The related object and its key column are written as one."""

    @pytest.mark.asyncio
    async def test_create_with_related_instance(self, db):
        author = await create_author(db)
        post = await Post.objects.create(
            title="By instance", author=author, using=db.name
        )
        fetched = await Post.objects.get(id=post.id, using=db.name)
        assert fetched.author_id == author.id

    @pytest.mark.asyncio
    async def test_reassign_and_save(self, db):
        first = await create_author(db)
        second = await create_author(db)
        post = await create_post(db, author_id=first.id)

        post.author = second
        await post.save(using=db.name)
        fetched = await Post.objects.get(id=post.id, using=db.name)
        assert fetched.author_id == second.id

        post.author = first
        await post.save(update_fields=["author"], using=db.name)
        fetched = await Post.objects.get(id=post.id, using=db.name)
        assert fetched.author_id == first.id

    @pytest.mark.asyncio
    async def test_detach_with_none(self, db):
        author = await create_author(db)
        category = await Category.objects.create(
            name=f"c-{author.id}", using=db.name
        )
        post = await create_post(db, author_id=author.id, category_id=category.id)

        post.category = None
        await post.save(using=db.name)
        fetched = await Post.objects.get(id=post.id, using=db.name)
        assert fetched.category_id is None

    @pytest.mark.asyncio
    async def test_related_object_saved_before_its_owner(self, db):
        author = Author(name="Later", email="later@test.com")
        post = Post(title="Graph", author=author)

        with pytest.raises(ManagerError, match="not saved yet"):
            await post.save(using=db.name)

        await author.save(using=db.name)
        await post.save(using=db.name)
        fetched = await Post.objects.get(id=post.id, using=db.name)
        assert fetched.author_id == author.id

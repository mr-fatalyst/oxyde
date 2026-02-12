from __future__ import annotations

import sqlite3
import uuid
from pathlib import Path

import pytest
import pytest_asyncio

from oxyde import AsyncDatabase, F, Field, Model, disconnect_all
from oxyde.models.registry import register_table


def _prepare_db(path: str | Path) -> None:
    path_obj = Path(path) if isinstance(path, str) else path
    path_obj.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(path)
    conn.execute("PRAGMA foreign_keys = ON")

    # Create all tables first
    conn.execute(
        "CREATE TABLE IF NOT EXISTS articles (id INTEGER PRIMARY KEY, title TEXT NOT NULL, views INTEGER NOT NULL)"
    )
    conn.execute(
        "CREATE TABLE IF NOT EXISTS authors (id INTEGER PRIMARY KEY, email TEXT NOT NULL, name TEXT NOT NULL)"
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS posts (
            id INTEGER PRIMARY KEY,
            title TEXT NOT NULL,
            author_id INTEGER NOT NULL,
            views INTEGER NOT NULL,
            FOREIGN KEY(author_id) REFERENCES authors(id)
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS comments (
            id INTEGER PRIMARY KEY,
            post_id INTEGER NOT NULL,
            body TEXT NOT NULL,
            FOREIGN KEY(post_id) REFERENCES posts(id)
        )
        """
    )

    # Delete in reverse FK order (child tables first)
    conn.execute("DELETE FROM comments")
    conn.execute("DELETE FROM posts")
    conn.execute("DELETE FROM authors")
    conn.execute("DELETE FROM articles")

    # Insert in FK order (parent tables first)
    conn.executemany(
        "INSERT INTO articles (id, title, views) VALUES (?, ?, ?)",
        [
            (1, "First", 10),
            (2, "Second", 5),
            (3, "Third", 20),
        ],
    )
    conn.executemany(
        "INSERT INTO authors (id, email, name) VALUES (?, ?, ?)",
        [
            (1, "ada@example.com", "Ada Lovelace"),
            (2, "linus@example.com", "Linus Torvalds"),
        ],
    )
    conn.executemany(
        "INSERT INTO posts (id, title, author_id, views) VALUES (?, ?, ?, ?)",
        [
            (1, "Rust Patterns", 1, 120),
            (2, "Async ORM", 1, 35),
            (3, "Kernel Notes", 2, 80),
        ],
    )
    conn.executemany(
        "INSERT INTO comments (id, post_id, body) VALUES (?, ?, ?)",
        [
            (1, 1, "Great read!"),
            (2, 1, "Thanks for sharing"),
            (3, 3, "Subscribed!"),
        ],
    )
    conn.commit()
    conn.close()


@pytest_asyncio.fixture
async def sqlite_db():
    db_path = "test.db"
    _prepare_db(db_path)

    db = AsyncDatabase(
        f"sqlite://{db_path}",
        name=f"test_{uuid.uuid4().hex}",
        overwrite=True,
    )
    await db.connect()

    try:
        yield db
    finally:
        await disconnect_all()


class TestArticleQueries:
    class Article(Model):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        views: int

        class Meta:
            is_table = True
            table_name = "articles"

    @pytest.mark.asyncio
    async def test_fetch_models(self, sqlite_db: AsyncDatabase) -> None:
        articles = await self.Article.objects.all(using=sqlite_db.name)
        titles = [article.title for article in articles]
        assert titles == ["First", "Second", "Third"]

    @pytest.mark.asyncio
    async def test_lookup_and_order(self, sqlite_db: AsyncDatabase) -> None:
        articles = await self.Article.objects.filter(views__gte=10).all(
            using=sqlite_db.name
        )
        assert [article.title for article in articles] == ["First", "Third"]

        ordered = await self.Article.objects.filter().order_by("-views").all(client=sqlite_db)
        assert [article.title for article in ordered] == ["Third", "First", "Second"]

    @pytest.mark.asyncio
    async def test_values_list_flat(self, sqlite_db: AsyncDatabase) -> None:
        titles = await self.Article.objects.values_list("title", flat=True).all(client=sqlite_db)
        assert titles == ["First", "Second", "Third"]


class TestArticleManagerHelpers:
    class Article(Model):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        views: int

        class Meta:
            is_table = True
            table_name = "articles"

    @pytest.mark.asyncio
    async def test_manager_shortcuts(self, sqlite_db: AsyncDatabase) -> None:
        article = await self.Article.objects.get(using=sqlite_db.name, id=1)
        assert article.title == "First"

        missing = await self.Article.objects.get_or_none(
            using=sqlite_db.name, title="Missing"
        )
        assert missing is None

        first = await self.Article.objects.first(using=sqlite_db.name)
        last = await self.Article.objects.last(using=sqlite_db.name)
        assert first.id == 1
        assert last.id == 3

        filtered_results = (
            await self.Article.objects.filter(views__gte=20)
            .limit(1)
            .all(using=sqlite_db.name)
        )
        assert filtered_results[0].id == 3

        count = await self.Article.objects.filter(views__gte=10).count(
            using=sqlite_db.name
        )
        assert count == 2


class TestArticleMutations:
    class Article(Model):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        views: int

        class Meta:
            is_table = True
            table_name = "articles"

    @pytest.mark.asyncio
    async def test_create_and_update(self, sqlite_db: AsyncDatabase) -> None:
        article = await self.Article.objects.create(
            using=sqlite_db.name,
            title="Created",
            views=1,
        )
        assert article.title == "Created"

        created_count = await self.Article.objects.filter(title="Created").count(
            using=sqlite_db.name
        )
        assert created_count == 1

        updated = await self.Article.objects.filter(title="Created").update(
            views=99,
            using=sqlite_db.name,
        )
        assert len(updated) == 1

        titles = await self.Article.objects.values_list("title", flat=True).all(client=sqlite_db)
        assert "Created" in titles

    @pytest.mark.asyncio
    async def test_delete(self, sqlite_db: AsyncDatabase) -> None:
        deleted = await self.Article.objects.filter(title="Second").delete(
            using=sqlite_db.name
        )
        assert deleted == 1

        remaining_query = self.Article.objects.values_list("title", flat=True)
        remaining = await remaining_query.all(client=sqlite_db)
        assert "Second" not in remaining

    @pytest.mark.asyncio
    async def test_bulk_create_and_expressions(self, sqlite_db: AsyncDatabase) -> None:
        # Delete all articles using filter without conditions
        await self.Article.objects.filter().delete(using=sqlite_db.name)

        new_rows = [
            {"title": "Bulk One", "views": 5},
            self.Article(title="Bulk Two", views=6),
        ]
        created = await self.Article.objects.bulk_create(new_rows, using=sqlite_db.name)

        assert [article.title for article in created] == ["Bulk One", "Bulk Two"]

        total = await self.Article.objects.count(using=sqlite_db.name)
        assert total == 2

        await self.Article.objects.filter(title="Bulk One").update(
            views=F("views") + 10,
            using=sqlite_db.name,
        )

        refreshed = await self.Article.objects.get(
            using=sqlite_db.name, title="Bulk One"
        )
        assert refreshed.views == 15


class TestRelationalQueries:
    class Author(Model):
        id: int | None = Field(default=None, db_pk=True)
        email: str
        name: str

        class Meta:
            is_table = True
            table_name = "authors"

    class Comment(Model):
        id: int | None = Field(default=None, db_pk=True)
        post_id: int = 0
        body: str = ""

        class Meta:
            is_table = True
            table_name = "comments"

    class Post(Model):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        author: Author | None = None
        views: int
        comments: list[Comment] = Field(db_reverse_fk="post_id")

        class Meta:
            is_table = True
            table_name = "posts"

    @pytest.mark.asyncio
    async def test_join_and_prefetch(self, sqlite_db: AsyncDatabase) -> None:
        # Re-register models in case they were cleared by other tests
        register_table(self.Author, overwrite=True)
        register_table(self.Comment, overwrite=True)
        register_table(self.Post, overwrite=True)

        query = self.Post.objects.join("author").prefetch("comments").order_by("-views")
        posts = await query.all(client=sqlite_db)

        assert [post.title for post in posts] == [
            "Rust Patterns",
            "Kernel Notes",
            "Async ORM",
        ]

        by_title = {post.title: post for post in posts}

        rust = by_title["Rust Patterns"]
        assert rust.author is not None
        assert rust.author.email == "ada@example.com"
        assert sorted(comment.body for comment in rust.comments) == [
            "Great read!",
            "Thanks for sharing",
        ]

        kernel = by_title["Kernel Notes"]
        assert kernel.author is not None
        assert kernel.author.name == "Linus Torvalds"
        assert [comment.body for comment in kernel.comments] == ["Subscribed!"]

        async_post = by_title["Async ORM"]
        assert async_post.author is not None
        assert async_post.comments == []

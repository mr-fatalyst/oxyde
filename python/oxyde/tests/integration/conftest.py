"""Integration test fixtures — real SQLite, real Rust core, full pipeline."""
from __future__ import annotations

import sqlite3
import uuid

import pytest
import pytest_asyncio

from oxyde import AsyncDatabase, Field, Model, disconnect_all
from oxyde.models.registry import register_table


# ── Models ──────────────────────────────────────────────────────────────


class Author(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(max_length=100)
    email: str = Field(db_unique=True)
    active: bool = Field(default=True)
    posts: list[Post] = Field(db_reverse_fk="author_id")

    class Meta:
        is_table = True
        table_name = "authors"


class Category(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(max_length=50, db_unique=True)

    class Meta:
        is_table = True
        table_name = "categories"


class Post(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str = Field(max_length=200)
    body: str = Field(default="")
    author: Author | None = None
    category: Category | None = Field(default=None, db_nullable=True)
    views: int = Field(default=0)
    published: bool = Field(default=False)
    comments: list[Comment] = Field(db_reverse_fk="post_id")
    tags: list[Tag] = Field(db_m2m=True, db_through="PostTag")

    class Meta:
        is_table = True
        table_name = "posts"


class Comment(Model):
    id: int | None = Field(default=None, db_pk=True)
    post: Post | None = None
    body: str = Field(default="")

    class Meta:
        is_table = True
        table_name = "comments"


class Tag(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(max_length=50, db_unique=True)

    class Meta:
        is_table = True
        table_name = "tags"


class PostTag(Model):
    id: int | None = Field(default=None, db_pk=True)
    post: Post | None = None
    tag: Tag | None = None

    class Meta:
        is_table = True
        table_name = "post_tags"


ALL_MODELS = [Author, Category, Post, Comment, Tag, PostTag]


# ── Schema & Seed SQL ───────────────────────────────────────────────────

SCHEMA_SQL = """\
PRAGMA foreign_keys = ON;

CREATE TABLE authors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    active INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE categories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE posts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    author_id INTEGER NOT NULL REFERENCES authors(id),
    category_id INTEGER REFERENCES categories(id),
    views INTEGER NOT NULL DEFAULT 0,
    published INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE comments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    post_id INTEGER NOT NULL REFERENCES posts(id),
    body TEXT NOT NULL DEFAULT ''
);

CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE post_tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    post_id INTEGER NOT NULL REFERENCES posts(id),
    tag_id INTEGER NOT NULL REFERENCES tags(id)
);
"""

SEED_SQL = """\
INSERT INTO authors (id, name, email, active) VALUES
    (1, 'Alice', 'alice@test.com', 1),
    (2, 'Bob', 'bob@test.com', 1),
    (3, 'Charlie', 'charlie@test.com', 0);

INSERT INTO categories (id, name) VALUES
    (1, 'Tech'),
    (2, 'Science');

INSERT INTO posts (id, title, body, author_id, category_id, views, published) VALUES
    (1, 'Rust Patterns',     '', 1, 1,   120, 1),
    (2, 'Async Python',      '', 1, 1,    35, 1),
    (3, 'Quantum Computing', '', 2, 2,    80, 1),
    (4, 'Draft Post',        '', 2, NULL,   0, 0),
    (5, 'ML Basics',         '', 3, 2,   200, 1),
    (6, 'Unpublished',       '', 3, NULL,   0, 0);

INSERT INTO comments (id, post_id, body) VALUES
    (1, 1, 'Great read!'),
    (2, 1, 'Thanks for sharing'),
    (3, 3, 'Interesting topic');

INSERT INTO tags (id, name) VALUES
    (1, 'Python'),
    (2, 'Rust'),
    (3, 'SQL');

INSERT INTO post_tags (post_id, tag_id) VALUES
    (1, 2),
    (1, 1),
    (2, 1),
    (3, 3);
"""


# ── Fixtures ────────────────────────────────────────────────────────────


@pytest.fixture(autouse=True)
def _register_models():
    """Re-register models (may be cleared by unit test cleanup)."""
    for model in ALL_MODELS:
        register_table(model, overwrite=True)


@pytest_asyncio.fixture
async def db(tmp_path):
    """Fresh SQLite DB with schema + seed for each test."""
    db_path = tmp_path / "test.db"

    conn = sqlite3.connect(db_path)
    conn.executescript(SCHEMA_SQL)
    conn.executescript(SEED_SQL)
    conn.close()

    database = AsyncDatabase(
        f"sqlite://{db_path}",
        name=f"test_{uuid.uuid4().hex}",
        overwrite=True,
    )
    await database.connect()
    try:
        yield database
    finally:
        await disconnect_all()


# ── Factories ───────────────────────────────────────────────────────────


async def create_author(db, **overrides):
    """Create an Author with sensible defaults."""
    defaults = {"name": "Test Author", "email": f"{uuid.uuid4().hex}@test.com"}
    defaults.update(overrides)
    return await Author.objects.create(**defaults, client=db)


async def create_post(db, *, author_id, **overrides):
    """Create a Post with sensible defaults."""
    defaults = {"title": "Test Post", "author_id": author_id}
    defaults.update(overrides)
    return await Post.objects.create(**defaults, client=db)


async def create_tag(db, **overrides):
    """Create a Tag with a unique name."""
    defaults = {"name": f"tag_{uuid.uuid4().hex[:8]}"}
    defaults.update(overrides)
    return await Tag.objects.create(**defaults, client=db)

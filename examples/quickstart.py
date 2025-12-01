"""Oxyde ORM Quickstart Example.

Demonstrates basic ORM usage:
- Database connection
- Model definition with relations
- CRUD operations via Manager API
- Joins and prefetch for related data

Usage:
    export DATABASE_URL=sqlite://demo.db
    python examples/quickstart.py
"""

from __future__ import annotations

import asyncio
import os

from oxyde import AsyncDatabase, OxydeModel, Field, disconnect_all


# =============================================================================
# Model Definitions
# =============================================================================

class Author(OxydeModel):
    """Author model with primary key."""

    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)

    class Meta:
        is_table = True
        table_name = "authors"


class Post(OxydeModel):
    """Post model with foreign key to Author."""

    id: int | None = Field(default=None, db_pk=True)
    title: str = Field(max_length=200)
    content: str = ""
    views: int = 0
    author: Author | None = Field(default=None, db_on_delete="CASCADE")

    class Meta:
        is_table = True
        table_name = "posts"


class Comment(OxydeModel):
    """Comment model with foreign key to Post."""

    id: int | None = Field(default=None, db_pk=True)
    post_id: int
    body: str
    likes: int = 0

    class Meta:
        is_table = True
        table_name = "comments"


# =============================================================================
# Database Setup (for demo purposes)
# =============================================================================

async def setup_tables(db: AsyncDatabase) -> None:
    """Create tables using raw SQL (in real apps, use migrations)."""
    await db.execute_raw("""
        CREATE TABLE IF NOT EXISTS authors (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE
        )
    """)
    await db.execute_raw("""
        CREATE TABLE IF NOT EXISTS posts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            content TEXT DEFAULT '',
            views INTEGER DEFAULT 0,
            author_id INTEGER REFERENCES authors(id) ON DELETE CASCADE
        )
    """)
    await db.execute_raw("""
        CREATE TABLE IF NOT EXISTS comments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            post_id INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
            body TEXT NOT NULL,
            likes INTEGER DEFAULT 0
        )
    """)


async def cleanup(db: AsyncDatabase) -> None:
    """Clean up test data."""
    await Comment.objects.filter().delete(using=db.name)
    await Post.objects.filter().delete(using=db.name)
    await Author.objects.filter().delete(using=db.name)


# =============================================================================
# Main Demo
# =============================================================================

async def main() -> None:
    # Connect to database
    url = os.getenv("DATABASE_URL", "sqlite://demo.db")
    db = AsyncDatabase(url, name="default")
    await db.connect()

    try:
        await setup_tables(db)
        await cleanup(db)

        print("=" * 60)
        print("Oxyde ORM Quickstart")
        print("=" * 60)

        # ---------------------------------------------------------------------
        # CREATE
        # ---------------------------------------------------------------------
        print("\n1. Creating records...")

        # Single create
        author = await Author.objects.create(
            using=db.name,
            name="Ada Lovelace",
            email="ada@example.com",
        )
        print(f"   Created author: {author.name} (id={author.id})")

        # Create with relation
        post = await Post.objects.create(
            using=db.name,
            title="Introduction to Algorithms",
            content="Let's explore the beauty of algorithms...",
            author_id=author.id,
        )
        print(f"   Created post: {post.title} (id={post.id})")

        # Bulk create
        comments = await Comment.objects.bulk_create(
            [
                {"post_id": post.id, "body": "Great article!"},
                {"post_id": post.id, "body": "Very helpful, thanks!"},
                Comment(post_id=post.id, body="Looking forward to more!"),
            ],
            using=db.name,
        )
        print(f"   Created {len(comments)} comments")

        # ---------------------------------------------------------------------
        # READ
        # ---------------------------------------------------------------------
        print("\n2. Reading records...")

        # Get single record
        fetched = await Author.objects.get(using=db.name, id=author.id)
        print(f"   Fetched author: {fetched.name}")

        # Get or None (no exception if not found)
        missing = await Author.objects.get_or_none(using=db.name, email="nobody@example.com")
        print(f"   Missing author: {missing}")

        # Filter with lookups
        active_comments = await Comment.objects.filter(likes__gte=0).all(using=db.name)
        print(f"   Found {len(active_comments)} comments with likes >= 0")

        # First/Last
        first_comment = await Comment.objects.first(using=db.name)
        print(f"   First comment: {first_comment.body[:30]}...")

        # Count
        total = await Comment.objects.count(using=db.name)
        print(f"   Total comments: {total}")

        # ---------------------------------------------------------------------
        # UPDATE
        # ---------------------------------------------------------------------
        print("\n3. Updating records...")

        # Update via filter
        updated = await Post.objects.filter(id=post.id).update(
            views=100,
            using=db.name,
        )
        print(f"   Updated {updated} post(s)")

        # Update via instance.save()
        author.name = "Augusta Ada King"
        await author.save(using=db.name)
        print(f"   Updated author name to: {author.name}")

        # ---------------------------------------------------------------------
        # DELETE
        # ---------------------------------------------------------------------
        print("\n4. Deleting records...")

        # Delete via filter
        deleted = await Comment.objects.filter(body__icontains="forward").delete(
            using=db.name,
        )
        print(f"   Deleted {deleted} comment(s)")

        # Check remaining
        remaining = await Comment.objects.count(using=db.name)
        print(f"   Remaining comments: {remaining}")

        # ---------------------------------------------------------------------
        # JOINS & PREFETCH
        # ---------------------------------------------------------------------
        print("\n5. Relations (join & prefetch)...")

        # Create more data for demo
        author2 = await Author.objects.create(
            using=db.name,
            name="Grace Hopper",
            email="grace@example.com",
        )
        post2 = await Post.objects.create(
            using=db.name,
            title="Debugging Stories",
            content="The first actual bug...",
            views=50,
            author_id=author2.id,
        )

        # Join to load related author
        posts = await Post.objects.join("author").all(using=db.name)
        for p in posts:
            author_name = p.author.name if p.author else "Unknown"
            print(f"   Post '{p.title}' by {author_name}")

        # ---------------------------------------------------------------------
        # VALUES & VALUES_LIST
        # ---------------------------------------------------------------------
        print("\n6. Projections (values/values_list)...")

        # Get only specific fields as dicts
        emails = await Author.objects.values("email").all(using=db.name)
        print(f"   Author emails: {[e['email'] for e in emails]}")

        # Get flat list of single field
        titles = await Post.objects.values_list("title", flat=True).all(using=db.name)
        print(f"   Post titles: {titles}")

        print("\n" + "=" * 60)
        print("Done!")
        print("=" * 60)

    finally:
        await disconnect_all()


if __name__ == "__main__":
    asyncio.run(main())

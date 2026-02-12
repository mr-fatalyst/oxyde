# First Project

Let's build a simple blog application to learn Oxyde's core features.

## Project Structure

```
blog/
├── models.py          # Database models
├── oxyde_config.py    # Oxyde configuration
├── migrations/        # Migration files
├── main.py            # Application logic
└── blog.db            # SQLite database (auto-created)
```

## Step 1: Initialize Project

Create a new directory and initialize Oxyde:

```bash
mkdir blog && cd blog
oxyde init
```

When prompted:

- Models module: `models`
- Dialect: `sqlite`
- Database URL: `sqlite:///blog.db`
- Migrations directory: `migrations`

## Step 2: Define Models

Create `models.py`:

```python
from datetime import datetime
from oxyde import Model, Field


class Author(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
    bio: str | None = Field(default=None)
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")

    class Meta:
        is_table = True
        table_name = "authors"


class Post(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    content: str
    published: bool = Field(default=False)
    views: int = Field(default=0)
    author: "Author" | None = Field(default=None, db_on_delete="CASCADE")
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")

    class Meta:
        is_table = True
        table_name = "posts"


class Tag(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(db_unique=True)

    class Meta:
        is_table = True
        table_name = "tags"
```

Key concepts:

- `table_name` overrides the default table name
- `db_default="CURRENT_TIMESTAMP"` sets a SQL default
- `author: "Author"` creates a foreign key relationship
- `db_on_delete="CASCADE"` deletes posts when author is deleted

## Step 3: Create and Apply Migrations

Generate migrations from models:

```bash
oxyde makemigrations
```

Apply to create tables:

```bash
oxyde migrate
```

## Step 4: Create the Application

Create `main.py`:

```python
import asyncio
from oxyde import db, F, Q
from models import Author, Post, Tag


async def main():
    # Connect to database
    await db.init(default="sqlite:///blog.db")

    try:
        # Create sample data
        await create_sample_data()

        # Run queries
        await demo_queries()

        # Show statistics
        await show_stats()
    finally:
        await db.close()


async def create_sample_data():
    """Create authors, posts, and tags."""
    print("Creating sample data...")

    # Create authors
    alice = await Author.objects.create(
        name="Alice Johnson",
        email="alice@example.com",
        bio="Python developer and tech writer"
    )

    bob = await Author.objects.create(
        name="Bob Smith",
        email="bob@example.com",
        bio="Backend engineer"
    )

    # Create posts
    await Post.objects.create(
        title="Getting Started with Oxyde",
        content="Oxyde is a high-performance async ORM...",
        published=True,
        author_id=alice.id
    )

    await Post.objects.create(
        title="Advanced Query Patterns",
        content="In this post, we explore advanced queries...",
        published=True,
        views=150,
        author_id=alice.id
    )

    await Post.objects.create(
        title="Draft: Performance Tips",
        content="Work in progress...",
        published=False,
        author_id=bob.id
    )

    # Create tags
    for name in ["python", "orm", "async", "tutorial"]:
        await Tag.objects.create(name=name)

    print("Sample data created!\n")


async def demo_queries():
    """Demonstrate various query patterns."""
    print("=== Query Examples ===\n")

    # Basic filtering
    published = await Post.objects.filter(published=True).all()
    print(f"Published posts: {len(published)}")

    # Multiple conditions with Q
    popular = await Post.objects.filter(
        Q(published=True) & Q(views__gte=100)
    ).all()
    print(f"Popular posts (100+ views): {len(popular)}")

    # Ordering and limiting
    recent = await Post.objects.filter(
        published=True
    ).order_by("-created_at").limit(5).all()
    print(f"Recent posts: {[p.title for p in recent]}")

    # Get single record
    author = await Author.objects.get(email="alice@example.com")
    print(f"Found author: {author.name}")

    # Update with F expression (atomic increment)
    await Post.objects.filter(title__contains="Oxyde").update(
        views=F("views") + 1
    )
    print("Incremented views for Oxyde posts")

    # Values (return dicts instead of models)
    emails = await Author.objects.values("name", "email").all()
    print(f"Author emails: {emails}")

    # Exists check
    has_drafts = await Post.objects.filter(published=False).exists()
    print(f"Has draft posts: {has_drafts}")

    print()


async def show_stats():
    """Show aggregate statistics."""
    print("=== Statistics ===\n")

    # Count
    total_posts = await Post.objects.count()
    print(f"Total posts: {total_posts}")

    # Sum
    total_views = await Post.objects.sum("views")
    print(f"Total views: {total_views}")

    # Average
    avg_views = await Post.objects.avg("views")
    print(f"Average views: {avg_views:.1f}")

    # Count with filter
    published_count = await Post.objects.filter(published=True).count()
    print(f"Published posts: {published_count}")


if __name__ == "__main__":
    asyncio.run(main())
```

## Step 5: Run the Application

```bash
python main.py
```

Expected output:

```
Creating sample data...
Sample data created!

=== Query Examples ===

Published posts: 2
Popular posts (100+ views): 1
Recent posts: ['Advanced Query Patterns', 'Getting Started with Oxyde']
Found author: Alice Johnson
Incremented views for Oxyde posts
Author emails: [{'name': 'Alice Johnson', 'email': 'alice@example.com'}, ...]
Has draft posts: True

=== Statistics ===

Total posts: 3
Total views: 151
Average views: 50.3
Published posts: 2
```

## Step 6: Add Transactions

For operations that must succeed or fail together, use transactions:

```python
from oxyde.db import transaction


async def transfer_post(post_id: int, new_author_id: int):
    """Transfer a post to a different author (atomic operation)."""
    async with transaction.atomic():
        # Get the post
        post = await Post.objects.get(id=post_id)

        # Verify new author exists
        new_author = await Author.objects.get(id=new_author_id)

        # Update the post
        post.author_id = new_author_id
        await post.save()

        print(f"Transferred '{post.title}' to {new_author.name}")
```

If any operation fails, the entire transaction rolls back.

## Step 7: Add Joins

Load related data efficiently:

```python
async def show_posts_with_authors():
    """Load posts with their authors in a single query."""
    posts = await Post.objects.join("author").filter(published=True).all()

    for post in posts:
        print(f"'{post.title}' by {post.author.name}")
```

## Next Steps

Now that you have a working project:

- [Models](../guide/models.md) — Learn all field types and options
- [Filtering](../guide/filtering.md) — Master the filter syntax
- [Transactions](../guide/transactions.md) — Understand transaction handling
- [Relations](../guide/relations.md) — Work with foreign keys and joins

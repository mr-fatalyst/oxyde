# Relations

Oxyde supports foreign key relationships and eager loading.

## Foreign Keys

### Defining a Foreign Key

Foreign keys are defined using type annotations:

```python
class Post(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: "Author" | None = Field(default=None, db_on_delete="CASCADE")

    class Meta:
        is_table = True
```

This creates:
- A column `author_id` in the database
- A foreign key constraint to `Author.id`

### Working with Foreign Keys

```python
# Create with FK ID
post = await Post.objects.create(
    title="Hello World",
    author_id=1  # Use the _id suffix
)

# Access FK value
print(post.author_id)  # 1
```

### FK to Non-PK Field

Reference a different field:

```python
class Resource(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    tenant: "Tenant" | None = Field(
        default=None,
        db_fk="uuid",  # Target Tenant.uuid instead of Tenant.id
        db_on_delete="CASCADE"
    )

    class Meta:
        is_table = True
```

This creates column `tenant_uuid` referencing `Tenant.uuid`.

### ON DELETE Actions

| Action | Description |
|--------|-------------|
| `CASCADE` | Delete related rows |
| `SET NULL` | Set FK to NULL |
| `RESTRICT` | Prevent deletion |
| `NO ACTION` | Deferred check |

```python
# CASCADE - delete posts when author deleted
author: "Author" | None = Field(default=None, db_on_delete="CASCADE")

# SET NULL - keep post, set author to NULL
author: "Author" | None = Field(default=None, db_on_delete="SET NULL")

# RESTRICT - prevent deletion if posts exist
author: "Author" | None = Field(default=None, db_on_delete="RESTRICT")
```

## Eager Loading

### join()

Load related models in a single query (LEFT JOIN):

```python
# Load author with each post
posts = await Post.objects.join("author").all()

for post in posts:
    print(f"{post.title} by {post.author.name}")
```

Without `join()`, accessing `post.author` would require another query.

### Multiple Joins

```python
posts = await Post.objects.join("author", "category").all()
```

### Nested Joins

Use `__` to traverse relations:

```python
# Post -> Author -> Company
posts = await Post.objects.join("author__company").all()
for post in posts:
    print(f"{post.title} by {post.author.name} at {post.author.company.name}")
```

## Reverse Relations

### Defining Reverse FK

On the "one" side of a one-to-many:

```python
class Author(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    posts: list["Post"] = Field(db_reverse_fk="author")  # Virtual field for reverse relation

    class Meta:
        is_table = True


class Post(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: "Author" | None = Field(default=None, db_on_delete="CASCADE")

    class Meta:
        is_table = True
```

### prefetch()

Load reverse relations:

```python
# Load all posts for each author
authors = await Author.objects.prefetch("posts").all()

for author in authors:
    print(f"{author.name} has {len(author.posts)} posts")
    for post in author.posts:
        print(f"  - {post.title}")
```

`prefetch()` executes a separate query and attaches results.

## Many-to-Many

### Defining M2M

Use a junction table:

```python
class Post(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    tags: list["Tag"] = Field(db_m2m=True, db_through="PostTag")

    class Meta:
        is_table = True


class Tag(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(db_unique=True)

    class Meta:
        is_table = True


class PostTag(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    post: "Post" | None = Field(default=None, db_on_delete="CASCADE")
    tag: "Tag" | None = Field(default=None, db_on_delete="CASCADE")

    class Meta:
        is_table = True
```

### Working with M2M

M2M relations are supported by `prefetch()`. The through model must have FK fields to both models:

```python
# Load posts with their tags
posts = await Post.objects.prefetch("tags").all()
for post in posts:
    print(f"{post.title}: {[t.name for t in post.tags]}")
```

You can also work with the junction table directly:

```python
# Add tag to post
await PostTag.objects.create(post_id=post.id, tag_id=tag.id)

# Remove tag from post
await PostTag.objects.filter(post_id=post.id, tag_id=tag.id).delete()

# Get all tags for a post (alternative approach)
post_tags = await PostTag.objects.filter(post_id=post.id).join("tag").all()
tags = [pt.tag for pt in post_tags]
```

## Automatic FK Column Naming

Oxyde automatically creates FK columns:

| Field Definition | Created Column |
|-----------------|----------------|
| `author: Author` | `author_id` |
| `author: Author` (PK is `uuid`) | `author_uuid` |
| `category: Category` (FK to `code`) | `category_code` |

## Common Patterns

### Self-Referential FK

```python
class Category(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    parent: "Category" | None = Field(default=None, db_on_delete="SET NULL")

    class Meta:
        is_table = True
```

### Polymorphic Relations (Manual)

```python
class Comment(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    content: str
    commentable_type: str  # "post" or "photo"
    commentable_id: int

    class Meta:
        is_table = True

# Query
post_comments = await Comment.objects.filter(
    commentable_type="post",
    commentable_id=post_id
).all()
```

### Soft Delete with Relations

```python
class Post(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    deleted_at: datetime | None = Field(default=None)
    author: "Author" | None = Field(default=None, db_on_delete="SET NULL")

    class Meta:
        is_table = True

# Query active posts with author
posts = await Post.objects.filter(
    deleted_at__isnull=True
).join("author").all()
```

## Complete Example

```python
from datetime import datetime
from oxyde import OxydeModel, Field, db

class Author(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    posts: list["Post"] = Field(db_reverse_fk="author")

    class Meta:
        is_table = True
        table_name = "authors"


class Post(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    content: str
    author: "Author" | None = Field(default=None, db_on_delete="CASCADE")
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")

    class Meta:
        is_table = True
        table_name = "posts"


async def main():
    async with db.connect("sqlite:///blog.db"):
        # Create author
        author = await Author.objects.create(name="Alice")

        # Create posts
        await Post.objects.create(
            title="First Post",
            content="Hello!",
            author_id=author.id
        )
        await Post.objects.create(
            title="Second Post",
            content="World!",
            author_id=author.id
        )

        # Load posts with author (JOIN)
        posts = await Post.objects.join("author").all()
        for post in posts:
            print(f"{post.title} by {post.author.name}")

        # Load author with posts (prefetch)
        authors = await Author.objects.prefetch("posts").all()
        for author in authors:
            print(f"{author.name}: {len(author.posts)} posts")
```

## Limitations

### No M2M in join()

M2M relations are supported by `prefetch()` but not by `join()`. Use `prefetch()` for M2M.

## Next Steps

- [Transactions](transactions.md) — Atomic operations
- [Queries](queries.md) — Full query reference
- [Fields](fields.md) — Field options reference

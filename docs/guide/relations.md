# Relations

Oxyde supports foreign key relationships and eager loading.

## Foreign Keys

### Defining a Foreign Key

Foreign keys are defined using type annotations:

```python
class Post(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: "Author" | None = Field(default=None, db_on_delete="CASCADE")
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
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    tenant: "Tenant" | None = Field(
        default=None,
        db_fk="uuid",  # Target Tenant.uuid instead of Tenant.id
        db_on_delete="CASCADE"
    )
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
# Load author and category
posts = await Post.objects.join("author").join("category").all()
```

## Reverse Relations

### Defining Reverse FK

On the "one" side of a one-to-many:

```python
class Author(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str

    # Virtual field for reverse relation
    posts: list["Post"] = Field(db_reverse_fk="author")


class Post(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: "Author" | None = Field(default=None, db_on_delete="CASCADE")
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
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    title: str
    tags: list["Tag"] = Field(db_m2m=True, db_through="PostTag")


class Tag(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(db_unique=True)


class PostTag(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    post: "Post" | None = Field(default=None, db_on_delete="CASCADE")
    tag: "Tag" | None = Field(default=None, db_on_delete="CASCADE")
```

### Working with M2M

Currently, M2M relations must be managed through the junction table:

```python
# Add tag to post
await PostTag.objects.create(post_id=post.id, tag_id=tag.id)

# Remove tag from post
await PostTag.objects.filter(post_id=post.id, tag_id=tag.id).delete()

# Get all tags for a post
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
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str
    parent: "Category" | None = Field(default=None, db_on_delete="SET NULL")
```

### Polymorphic Relations (Manual)

```python
class Comment(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    content: str
    commentable_type: str  # "post" or "photo"
    commentable_id: int

# Query
post_comments = await Comment.objects.filter(
    commentable_type="post",
    commentable_id=post_id
).all()
```

### Soft Delete with Relations

```python
class Post(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    title: str
    deleted_at: datetime | None = Field(default=None)
    author: "Author" | None = Field(default=None, db_on_delete="SET NULL")

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
    class Meta:
        is_table = True
        table_name = "authors"

    id: int | None = Field(default=None, db_pk=True)
    name: str
    posts: list["Post"] = Field(db_reverse_fk="author")


class Post(OxydeModel):
    class Meta:
        is_table = True
        table_name = "posts"

    id: int | None = Field(default=None, db_pk=True)
    title: str
    content: str
    author: "Author" | None = Field(default=None, db_on_delete="CASCADE")
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")


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

### No Nested Joins

Oxyde doesn't support nested joins like `join("author.company")`. Query separately:

```python
# Instead of: Post.objects.join("author.company")

posts = await Post.objects.join("author").all()
author_ids = [p.author_id for p in posts if p.author_id]
companies = await Company.objects.filter(id__in=author_ids).all()
```

### No Automatic M2M Loading

M2M relations require manual junction table queries.

## Next Steps

- [Transactions](transactions.md) — Atomic operations
- [Queries](queries.md) — Full query reference
- [Fields](fields.md) — Field options reference

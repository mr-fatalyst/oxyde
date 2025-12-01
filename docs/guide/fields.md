# Fields

The `Field()` function configures both Pydantic validation and database schema.

## Basic Usage

```python
from oxyde import OxydeModel, Field

class User(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(max_length=100)
    email: str = Field(db_unique=True, db_index=True)
```

## Field() Parameters

### Database Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `db_pk` | `bool` | `False` | Primary key |
| `db_index` | `bool` | `False` | Create index |
| `db_index_name` | `str` | Auto | Custom index name |
| `db_index_method` | `str` | `"btree"` | Index method: btree, hash, gin, gist |
| `db_unique` | `bool` | `False` | UNIQUE constraint |
| `db_column` | `str` | Field name | Database column name |
| `db_type` | `str` | Auto | SQL type override |
| `db_default` | `str` | None | SQL DEFAULT expression |
| `db_comment` | `str` | None | Column comment |

### Foreign Key Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `db_fk` | `str` | PK of related model | Target field for FK |
| `db_on_delete` | `str` | `"RESTRICT"` | ON DELETE action |
| `db_on_update` | `str` | `"CASCADE"` | ON UPDATE action |

### Relation Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `db_reverse_fk` | `str` | None | Reverse FK field name |
| `db_m2m` | `bool` | `False` | Many-to-many relation |
| `db_through` | `str` | None | M2M junction table |

### Pydantic Parameters

All standard Pydantic `Field()` parameters work:

| Parameter | Type | Description |
|-----------|------|-------------|
| `default` | Any | Default value |
| `default_factory` | Callable | Factory for default value |
| `alias` | `str` | JSON key name |
| `description` | `str` | Field description |
| `ge`, `gt`, `le`, `lt` | Number | Numeric bounds |
| `min_length`, `max_length` | `int` | String length bounds |
| `pattern` | `str` | Regex pattern |

## Primary Key

```python
# Auto-increment integer
id: int | None = Field(default=None, db_pk=True)

# UUID
from uuid import UUID, uuid4
id: UUID = Field(default_factory=uuid4, db_pk=True)

# Custom type
id: int = Field(db_pk=True, db_type="BIGSERIAL")
```

## Indexes

```python
# Simple index
email: str = Field(db_index=True)

# Unique index
username: str = Field(db_unique=True)

# Custom index name
email: str = Field(db_index=True, db_index_name="ix_users_email")

# Index method (PostgreSQL)
data: dict = Field(db_type="JSONB", db_index=True, db_index_method="gin")
```

## SQL Defaults

```python
# Timestamp
created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")

# PostgreSQL functions
uuid: str = Field(db_default="gen_random_uuid()")

# String literal (note the quotes)
status: str = Field(db_default="'pending'")

# Numeric
count: int = Field(db_default="0")
```

!!! note "Python vs SQL Default"
    `default` is used when creating Python objects.
    `db_default` is used by the database when inserting rows.

## Column Mapping

```python
# Different Python name and DB column
created_at: datetime = Field(db_column="created_timestamp")

# With JSON alias too
created_at: datetime = Field(
    alias="createdAt",           # JSON API uses camelCase
    db_column="created_timestamp" # DB uses snake_case
)
```

## Custom SQL Types

```python
# Override inferred type
name: str = Field(db_type="VARCHAR(255)")

# PostgreSQL-specific
data: dict = Field(db_type="JSONB")
tags: list[str] = Field(db_type="TEXT[]")

# MySQL-specific
content: str = Field(db_type="LONGTEXT")
```

## Foreign Keys

Foreign keys are defined by type annotation:

```python
class Post(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    title: str

    # FK to Author (creates author_id column)
    author: "Author" | None = Field(default=None, db_on_delete="CASCADE")
```

### FK to Non-PK Field

```python
# FK to Author.uuid instead of Author.id
author: "Author" | None = Field(
    default=None,
    db_fk="uuid",  # Target the uuid field
    db_on_delete="CASCADE"
)
# Creates author_uuid column
```

### ON DELETE Actions

| Action | Description |
|--------|-------------|
| `CASCADE` | Delete related rows |
| `SET NULL` | Set FK to NULL (requires nullable field) |
| `RESTRICT` | Prevent deletion if references exist |
| `NO ACTION` | Same as RESTRICT (deferred) |

```python
# CASCADE - delete posts when author is deleted
author: "Author" | None = Field(default=None, db_on_delete="CASCADE")

# SET NULL - set author_id to NULL when author is deleted
author: "Author" | None = Field(default=None, db_on_delete="SET NULL")

# RESTRICT - prevent author deletion if posts exist
author: "Author" | None = Field(default=None, db_on_delete="RESTRICT")
```

## Relations

### Reverse Foreign Key

Define on the "one" side of a one-to-many relationship:

```python
class Author(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str

    # Virtual field - not stored in DB
    posts: list["Post"] = Field(db_reverse_fk="author")
```

Use with `prefetch()`:

```python
authors = await Author.objects.prefetch("posts").all()
for author in authors:
    print(f"{author.name} has {len(author.posts)} posts")
```

### Many-to-Many

```python
class Post(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    title: str

    # M2M through junction table
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

## Pydantic Validation

### Numeric Bounds

```python
age: int = Field(ge=0, le=150)      # 0 <= age <= 150
price: float = Field(gt=0)          # price > 0
quantity: int = Field(ge=1, le=100) # 1 <= quantity <= 100
```

### String Validation

```python
name: str = Field(min_length=1, max_length=100)
email: str = Field(pattern=r"^[\w.-]+@[\w.-]+\.\w+$")
```

### Required vs Optional

```python
# Required - no default
name: str

# Optional with None default
bio: str | None = Field(default=None)

# Optional with value default
status: str = Field(default="active")

# Required but can be None
data: str | None  # Must be passed, but can be None
```

## Comments

Add SQL comments to columns:

```python
email: str = Field(
    db_unique=True,
    db_comment="Primary email address for notifications"
)
```

## Complete Example

```python
from datetime import datetime
from decimal import Decimal
from uuid import UUID, uuid4
from oxyde import OxydeModel, Field

class Product(OxydeModel):
    class Meta:
        is_table = True
        table_name = "products"

    # Primary key
    id: UUID = Field(default_factory=uuid4, db_pk=True)

    # Required fields
    name: str = Field(min_length=1, max_length=200)
    price: Decimal = Field(ge=0, db_type="NUMERIC(10, 2)")

    # Optional fields
    description: str | None = Field(default=None)
    sku: str | None = Field(default=None, db_unique=True, db_index=True)

    # With defaults
    active: bool = Field(default=True)
    stock: int = Field(default=0, ge=0)

    # SQL defaults
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")
    updated_at: datetime | None = Field(default=None)

    # Foreign key
    category: "Category" | None = Field(default=None, db_on_delete="SET NULL")
```

## Next Steps

- [Queries](queries.md) — Query your models
- [Filtering](filtering.md) — Filter expressions
- [Relations](relations.md) — Foreign keys and joins

# Models

Models are the foundation of Oxyde. Each model class represents a database table, with class attributes defining columns.

## Basic Model Definition

```python
from oxyde import Model, Field

class User(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
    age: int | None = Field(default=None)

    class Meta:
        is_table = True
```

## The Meta Class

The inner `Meta` class configures table-level settings:

```python
class User(Model):
    class Meta:
        is_table = True              # Required: marks this as a database table
        table_name = "users"         # Optional: custom table name (default: class name)
        schema = "public"            # Optional: database schema
```

### Required Settings

| Setting | Type | Description |
|---------|------|-------------|
| `is_table` | `bool` | Must be `True` for database tables |

### Optional Settings

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `table_name` | `str` | Class name | Database table name |
| `schema` | `str` | None | Database schema |
| `indexes` | `list[Index]` | `[]` | Composite indexes |
| `constraints` | `list[Check]` | `[]` | CHECK constraints |
| `unique_together` | `list[tuple]` | `[]` | Composite unique constraints |
| `primary_key` | `tuple[str, ...]` | None | Composite primary key |

## Type Annotations

Oxyde uses Python type hints to infer SQL types:

```python
class Example(Model):
    # Required field
    name: str

    # Optional field (nullable)
    bio: str | None = Field(default=None)

    # With default value
    status: str = Field(default="active")

    class Meta:
        is_table = True
```

### Type Mapping

| Python Type | PostgreSQL | SQLite | MySQL |
|-------------|------------|--------|-------|
| `int` | BIGINT | INTEGER | BIGINT |
| `str` | TEXT | TEXT | TEXT |
| `float` | DOUBLE PRECISION | REAL | DOUBLE |
| `bool` | BOOLEAN | INTEGER | TINYINT |
| `datetime` | TIMESTAMP | TEXT | DATETIME |
| `date` | DATE | TEXT | DATE |
| `UUID` | UUID | TEXT | CHAR(36) |
| `Decimal` | NUMERIC | NUMERIC | DECIMAL |
| `bytes` | BYTEA | BLOB | BLOB |

## Primary Keys

### Auto-increment Primary Key

```python
class User(Model):
    id: int | None = Field(default=None, db_pk=True)

    class Meta:
        is_table = True
```

The `id` will be auto-generated on insert.

### UUID Primary Key

```python
from uuid import UUID, uuid4

class User(Model):
    id: UUID = Field(default_factory=uuid4, db_pk=True)

    class Meta:
        is_table = True
```

### Composite Primary Key

```python
class UserRole(Model):
    user_id: int
    role_id: int

    class Meta:
        is_table = True
        primary_key = ("user_id", "role_id")
```

## Indexes

### Single-Column Index

```python
class User(Model):
    email: str = Field(db_index=True)

    class Meta:
        is_table = True
```

### Composite Index

```python
from oxyde import Index

class Event(Model):
    city: str
    start_date: datetime

    class Meta:
        is_table = True
        indexes = [
            Index(("city", "start_date")),
        ]
```

### Partial Index

```python
class User(Model):
    email: str
    deleted_at: datetime | None = Field(default=None)

    class Meta:
        is_table = True
        indexes = [
            Index(("email",), unique=True, where="deleted_at IS NULL"),
        ]
```

### Index Methods

PostgreSQL supports different index methods:

```python
Index(("data",), method="gin")   # GIN index for JSONB
Index(("name",), method="hash")  # Hash index for equality
```

## Constraints

### UNIQUE Constraint

```python
# Single column
email: str = Field(db_unique=True)

# Multiple columns
class Meta:
    unique_together = [("user_id", "slug")]
```

### CHECK Constraint

```python
from oxyde import Check

class Event(Model):
    start_date: datetime
    end_date: datetime
    price: float

    class Meta:
        is_table = True
        constraints = [
            Check("start_date < end_date", name="valid_dates"),
            Check("price >= 0"),
        ]
```

## SQL Defaults

Set database-level default values:

```python
class User(Model):
    id: int | None = Field(default=None, db_pk=True)
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")
    uuid: str = Field(db_default="gen_random_uuid()")  # PostgreSQL
    status: str = Field(db_default="'active'")  # Note: strings need quotes

    class Meta:
        is_table = True
```

!!! warning "Python vs SQL Defaults"
    - `default=value` — Python-side default, used when creating instances
    - `db_default="..."` — SQL-side default, used by the database

## Column Mapping

Override the database column name:

```python
class User(Model):
    created_at: datetime = Field(db_column="created_timestamp")

    class Meta:
        is_table = True
```

The Python attribute is `created_at`, but the database column is `created_timestamp`.

## Custom SQL Types

Override the inferred SQL type:

```python
class User(Model):
    id: int = Field(db_pk=True, db_type="BIGSERIAL")
    name: str = Field(db_type="VARCHAR(255)")
    data: dict = Field(db_type="JSONB")  # PostgreSQL

    class Meta:
        is_table = True
```

## Instance Methods

### save()

Insert or update a record:

```python
# Insert new record
user = User(name="Alice", email="alice@example.com")
await user.save()
print(user.id)  # Auto-generated ID

# Update existing record
user.name = "Alice Smith"
await user.save()

# Partial update (only specified fields)
user.age = 31
await user.save(update_fields=["age"])
```

### delete()

Delete a record:

```python
user = await User.objects.get(id=1)
await user.delete()
```

### refresh()

Reload from database:

```python
user = await User.objects.get(id=1)
# ... some other process updates the database ...
await user.refresh()  # Reload latest data
```

## Lifecycle Hooks

Override these methods to run code before/after database operations:

```python
class User(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str
    created_at: datetime | None = Field(default=None)
    updated_at: datetime | None = Field(default=None)

    class Meta:
        is_table = True

    async def pre_save(self, *, is_create: bool, update_fields: list[str] | None = None):
        """Called before save()."""
        from datetime import datetime
        now = datetime.utcnow()
        if is_create:
            self.created_at = now
        self.updated_at = now

    async def post_save(self, *, is_create: bool, update_fields: list[str] | None = None):
        """Called after save()."""
        if is_create:
            print(f"Created user {self.id}")

    async def pre_delete(self):
        """Called before delete()."""
        print(f"About to delete user {self.id}")

    async def post_delete(self):
        """Called after delete()."""
        print(f"Deleted user {self.id}")
```

## Model Inheritance

### Abstract Models

Create base models without database tables:

```python
class TimestampMixin(Model):
    """Mixin for created_at/updated_at fields."""
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")
    updated_at: datetime | None = Field(default=None)


class User(TimestampMixin):
    id: int | None = Field(default=None, db_pk=True)
    name: str

    class Meta:
        is_table = True
```

Only `User` creates a database table.

## Pydantic Integration

Model inherits from Pydantic's BaseModel, so you get:

### Validation

```python
class User(Model):
    id: int | None = Field(default=None, db_pk=True)
    age: int = Field(ge=0, le=150)  # Must be 0-150
    email: str = Field(pattern=r"^[\w.-]+@[\w.-]+\.\w+$")

    class Meta:
        is_table = True

# Raises ValidationError
user = User(age=200, email="invalid")
```

### Serialization

```python
user = await User.objects.get(id=1)

# To dict
data = user.model_dump()

# To JSON
json_str = user.model_dump_json()

# From dict
user = User.model_validate({"name": "Alice", "email": "alice@example.com"})
```

### JSON Aliases

```python
class User(Model):
    created_at: datetime = Field(
        alias="createdAt",        # JSON key
        db_column="created_at",   # Database column
    )

    class Meta:
        is_table = True
```

## Next Steps

- [Fields](fields.md) — Complete field reference
- [Queries](queries.md) — Query your models
- [Relations](relations.md) — Foreign keys and joins

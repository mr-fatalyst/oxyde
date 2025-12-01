# Oxyde ORM

High-performance async Python ORM with Rust core.

Oxyde combines Python's expressiveness with Rust's performance. Models defined with Pydantic v2, queries executed in native Rust.

```python
from oxyde import OxydeModel, Field, db

class User(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    email: str = Field(db_unique=True)
    age: int = Field(ge=0, le=150)

async def main():
    async with db.connect("postgresql://localhost/mydb"):
        # Create
        user = await User.objects.create(email="alice@example.com", age=30)

        # Read
        users = await User.objects.filter(age__gte=18).limit(10).all()

        # Update
        await User.objects.filter(id=user.id).update(age=31)

        # Delete
        await User.objects.filter(id=user.id).delete()
```

## Features

- **Django-style API** — Familiar `Model.objects.filter()` syntax
- **Pydantic v2 models** — Full validation, type hints, serialization
- **Async-first** — Built for modern async Python with `asyncio`
- **Rust performance** — SQL generation and execution in native Rust
- **Multi-database** — PostgreSQL, SQLite, MySQL support
- **Transactions** — `atomic()` context manager with savepoints
- **Migrations** — Django-style `makemigrations` and `migrate` CLI

## Installation

```bash
pip install oxyde
```

## Quick Start

### Define a Model

```python
from oxyde import OxydeModel, Field

class User(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
    age: int | None = Field(default=None)
```

### Connect and Query

```python
from oxyde import db

async with db.connect("sqlite:///app.db"):
    # Create
    user = await User.objects.create(name="Alice", email="alice@example.com", age=30)

    # Query
    adults = await User.objects.filter(age__gte=18).all()

    # Get single object
    user = await User.objects.get(id=1)

    # Update
    user.age = 31
    await user.save()

    # Delete
    await user.delete()
```

### Transactions

```python
from oxyde.db import transaction

async with transaction.atomic():
    user = await User.objects.create(name="Alice", email="alice@example.com")
    await Profile.objects.create(user_id=user.id)
    # Auto-commits on success, rolls back on exception
```

### FastAPI Integration

```python
from fastapi import FastAPI
from oxyde import db

app = FastAPI(
    lifespan=db.lifespan(
        default="postgresql://localhost/mydb",
    )
)

@app.get("/users")
async def get_users():
    return await User.objects.filter(is_active=True).all()
```

## Database Support

| Database   | Min Version | Status | Notes |
|------------|-------------|--------|-------|
| PostgreSQL | 12+ | Full | RETURNING, UPSERT, FOR UPDATE/SHARE, JSON, Arrays |
| SQLite     | 3.35+ | Full | RETURNING, UPSERT, WAL mode by default |
| MySQL      | 8.0+ | Full | UPSERT via ON DUPLICATE KEY, FOR UPDATE/SHARE |

**Recommendation**: PostgreSQL for production, SQLite for development/testing.

> **SQLite < 3.35**: Falls back to `last_insert_rowid()` which may return incorrect IDs with concurrent inserts.
>
> **MySQL**: No RETURNING clause — uses `last_insert_id()`. Bulk INSERT returns calculated ID range which may be incorrect with concurrent inserts.

**Connection URLs:**

```python
"postgresql://user:password@localhost:5432/database"
"sqlite:///path/to/database.db"
"sqlite:///:memory:"
"mysql://user:password@localhost:3306/database"
```

## Documentation

Full documentation: **[https://oxyde.fatalyst.dev/](https://oxyde.fatalyst.dev/)**

- [Getting Started](https://oxyde.fatalyst.dev/getting-started/quickstart/) — First steps with Oxyde
- [User Guide](https://oxyde.fatalyst.dev/guide/models/) — Models, queries, relations, transactions
- [API Reference](https://oxyde.fatalyst.dev/reference/api/models/) — Complete API documentation
- [Cheatsheet](https://oxyde.fatalyst.dev/reference/cheatsheet/) — Quick reference for all methods
- [API Classification](ORM_API_CLASSIFICATION.md) — Complete API table with examples

## Contributing

If you have suggestions or find a bug, please open an issue or create a pull request on GitHub.

## License

This project is licensed under the terms of the MIT license.

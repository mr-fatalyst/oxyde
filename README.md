<p align="center">
  <img src="https://raw.githubusercontent.com/mr-fatalyst/oxyde/master/logo.png" alt="Logo" width="200">
</p>

<p align="center"> <b>Oxyde ORM</b> is a type-safe, Pydantic-centric asynchronous ORM with a high-performance Rust core designed for clarity, speed, and reliability. </p>

<p align="center"> Inspired by the elegance of <a href="https://www.djangoproject.com/">Django's ORM</a>, Oxyde focuses on explicitness over magic, providing a modern developer-friendly workflow with predictable behavior and strong typing throughout. </p>

<p align="center">
  <img src="https://img.shields.io/github/license/mr-fatalyst/oxyde">
  <img src="https://github.com/mr-fatalyst/oxyde/actions/workflows/test.yml/badge.svg">
  <img src="https://img.shields.io/pypi/v/oxyde">
  <img src="https://img.shields.io/pypi/pyversions/oxyde">
  <img src="https://static.pepy.tech/badge/oxyde" alt="PyPI Downloads">
</p>

---

## Features

- **Django-style API** — Familiar `Model.objects.filter()` syntax
- **Pydantic v2 models** — Full validation, type hints, serialization
- **Async-first** — Built for modern async Python with `asyncio`
- **Rust performance** — SQL generation and execution in native Rust
- **Multi-database** — PostgreSQL, SQLite, MySQL support
- **Transactions** — `transaction.atomic()` context manager with savepoints
- **Migrations** — Django-style `makemigrations` and `migrate` CLI

## Installation

```bash
pip install oxyde
```

## Quick Start

### 1. Initialize Project

```bash
oxyde init
```

This creates `oxyde_config.py` with your database settings and model paths.

### 2. Define Models

```python
# models.py
from oxyde import OxydeModel, Field

class User(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
    age: int | None = Field(default=None)

    class Meta:
        is_table = True
```

### 3. Create Tables

```bash
oxyde makemigrations
oxyde migrate
```

### 4. Use It

```python
import asyncio
from oxyde import db
from models import User

async def main():
    await db.init(default="sqlite:///app.db")

    # Create
    user = await User.objects.create(name="Alice", email="alice@example.com", age=30)

    # Read
    users = await User.objects.filter(age__gte=18).all()
    user = await User.objects.get(id=1)

    # Update
    user.age = 31
    await user.save()

    # Delete
    await user.delete()

    await db.close()

asyncio.run(main())
```

## Transactions

```python
from oxyde.db import transaction

async with transaction.atomic():
    user = await User.objects.create(name="Alice", email="alice@example.com")
    await Profile.objects.create(user_id=user.id)
    # Auto-commits on success, rolls back on exception
```

## FastAPI Integration

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

**Connection URLs:**

```
postgresql://user:password@localhost:5432/database
sqlite:///path/to/database.db
sqlite:///:memory:
mysql://user:password@localhost:3306/database
```

## Documentation

Full documentation: **[https://oxyde.fatalyst.dev/](https://oxyde.fatalyst.dev/)**

- [Quick Start](https://oxyde.fatalyst.dev/latest/getting-started/quickstart/) — Get up and running
- [User Guide](https://oxyde.fatalyst.dev/latest/guide/models/) — Models, queries, relations, transactions
- [Cheatsheet](https://oxyde.fatalyst.dev/latest/cheatsheet/) — Quick reference for all methods

## Contributing

If you have suggestions or find a bug, please open an issue or create a pull request on GitHub.

## License

This project is licensed under the terms of the MIT license.

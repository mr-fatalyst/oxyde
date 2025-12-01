# Oxyde ORM

High-performance async Python ORM with Rust core.

Oxyde combines Python's expressiveness with Rust's performance. Models are defined using Pydantic v2, queries execute in native Rust. Communication happens via MessagePack protocol with ~2KB binary payloads.

```python
from oxyde import OxydeModel, Field, db, Q, F

class User(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    email: str = Field(db_unique=True)
    age: int = Field(ge=0, le=150)

async def main():
    await db.init(default="postgresql://localhost/mydb")

    # Create
    user = await User.objects.create(email="alice@example.com", age=30)

    # Query with Django-style filters
    users = await User.objects.filter(age__gte=18).limit(10).all()

    # Atomic update
    await User.objects.filter(id=1).update(views=F("views") + 1)

    await db.close()
```

## Features

- **Django-style API** — Familiar `Model.objects.filter()` syntax
- **Pydantic v2 models** — Full validation, type hints, serialization
- **Async-first** — Built for modern async Python with `asyncio`
- **Rust performance** — SQL generation and execution in native Rust
- **Multi-database** — PostgreSQL, SQLite, MySQL support
- **Transactions** — `atomic()` context manager with savepoints
- **Migrations** — Django-style `makemigrations` and `migrate` CLI

## Database Support

| Database   | Min Version | Status | Notes |
|------------|-------------|--------|-------|
| PostgreSQL | 8.2+ | Full | RETURNING, UPSERT, FOR UPDATE/SHARE, JSON, Arrays |
| SQLite     | 3.35+ | Full | RETURNING, UPSERT, WAL mode by default |
| MySQL      | 5.7+ | Full | UPSERT via ON DUPLICATE KEY, FOR UPDATE/SHARE |

> **SQLite < 3.35**: Falls back to `last_insert_rowid()` which may return incorrect IDs with concurrent inserts.
>
> **MySQL**: No RETURNING clause — uses `last_insert_id()`. Bulk INSERT returns calculated ID range which may be incorrect with concurrent inserts.

## Quick Links

<div class="grid cards" markdown>

-   :material-download:{ .lg .middle } **Installation**

    ---

    Install Oxyde and set up your environment

    [:octicons-arrow-right-24: Getting started](getting-started/installation.md)

-   :material-rocket-launch:{ .lg .middle } **Quick Start**

    ---

    Build your first app in 5 minutes

    [:octicons-arrow-right-24: Quick start](getting-started/quickstart.md)

-   :material-book-open-variant:{ .lg .middle } **Guide**

    ---

    Learn Oxyde step by step

    [:octicons-arrow-right-24: User guide](guide/models.md)

-   :material-api:{ .lg .middle } **API Reference**

    ---

    Complete API documentation

    [:octicons-arrow-right-24: Cheatsheet](cheatsheet.md)

</div>

## Why Oxyde?

### Performance

Oxyde's Rust core handles SQL generation and query execution, releasing Python's GIL during database I/O. This enables true parallelism for database operations.

### Type Safety

Built on Pydantic v2, Oxyde provides full type checking for models and queries. Your IDE understands your database schema.

### Familiar API

If you know Django ORM, you know Oxyde. The QuerySet API follows Django conventions:

```python
# Django
User.objects.filter(age__gte=18).exclude(status="banned").order_by("-created_at")

# Oxyde (identical)
await User.objects.filter(age__gte=18).exclude(status="banned").order_by("-created_at").all()
```

### Async Native

No sync wrappers or thread pools. Oxyde is async from the ground up:

```python
async with atomic():
    user = await User.objects.create(name="Alice")
    await Profile.objects.create(user_id=user.id)
```

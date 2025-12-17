# Oxyde ORM

<p align="center">
  <img src="https://raw.githubusercontent.com/mr-fatalyst/oxyde/master/logo.png" alt="Logo" width="200">
</p>

<p align="center"> <b>Oxyde ORM</b> is a type-safe, Pydantic-centric asynchronous ORM with a high-performance Rust core designed for clarity, speed, and reliability. </p>

<p align="center"> Inspired by the elegance of <a href="https://www.djangoproject.com/">Django’s ORM</a>, Oxyde focuses on explicitness over magic, providing a modern developer-friendly workflow with predictable behavior and strong typing throughout. </p>

<p align="center">
  <img src="https://img.shields.io/github/license/mr-fatalyst/oxyde">
  <img src="https://github.com/mr-fatalyst/oxyde/actions/workflows/test.yml/badge.svg">
  <img src="https://img.shields.io/pypi/v/oxyde">
  <img src="https://img.shields.io/pypi/pyversions/oxyde">
  <img src="https://static.pepy.tech/badge/oxyde" alt="PyPI Downloads">
</p>

---

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

## Features

- **Django-style API** — Familiar `Model.objects.filter()` syntax
- **Pydantic v2 models** — Full validation, type hints, serialization
- **Async-first** — Built for modern async Python with `asyncio`
- **Rust performance** — SQL generation and execution in native Rust
- **Multi-database** — PostgreSQL, SQLite, MySQL support
- **Transactions** — `transaction.atomic()` context manager with savepoints
- **Migrations** — Django-style `makemigrations` and `migrate` CLI

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
from oxyde.db import transaction

async with transaction.atomic():
    user = await User.objects.create(name="Alice")
    await Profile.objects.create(user_id=user.id)
```

## Database Support

| Database   | Min Version | Status | Notes |
|------------|-------------|--------|-------|
| PostgreSQL | 12+ | Full | RETURNING, UPSERT, FOR UPDATE/SHARE, JSON, Arrays |
| SQLite     | 3.35+ | Full | RETURNING, UPSERT, WAL mode by default |
| MySQL      | 8.0+ | Full | UPSERT via ON DUPLICATE KEY, FOR UPDATE/SHARE |

> **SQLite < 3.35**: Falls back to `last_insert_rowid()` which may return incorrect IDs with concurrent inserts.
>
> **MySQL**: No RETURNING clause — uses `last_insert_id()`. Bulk INSERT returns calculated ID range which may be incorrect with concurrent inserts.

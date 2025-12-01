# Migrations

Oxyde provides Django-style migrations for schema management.

## Overview

Migrations track database schema changes:

1. Define models in Python
2. Run `oxyde makemigrations` to generate migration files
3. Run `oxyde migrate` to apply changes to the database

## CLI Commands

### makemigrations

Generate migration files from model changes:

```bash
# Generate migrations
oxyde makemigrations

# With custom name
oxyde makemigrations --name "add_user_profile"

# Dry run (show without creating)
oxyde makemigrations --dry-run

# Custom migrations directory
oxyde makemigrations --migrations-dir ./db/migrations

# Specify dialect
oxyde makemigrations --dialect postgres
```

### migrate

Apply pending migrations:

```bash
# Apply all pending
oxyde migrate

# Custom migrations directory
oxyde migrate --migrations-dir ./db/migrations

# Target specific migration
oxyde migrate 0003

# Rollback to specific migration
oxyde migrate 0001
```

### showmigrations

List migration status:

```bash
oxyde showmigrations
```

Output:

```
[X] 0001_initial.py
[X] 0002_add_profile.py
[ ] 0003_add_tags.py
```

- `[X]` = Applied
- `[ ]` = Pending

### sqlmigration

Show SQL for a migration without running it:

```bash
oxyde sqlmigration myapp 0001
```

## Migration Files

Migrations are Python files in the `migrations/` directory:

```
migrations/
├── 0001_initial.py
├── 0002_add_profile.py
└── 0003_add_tags.py
```

### Migration Structure

```python
# 0001_initial.py

dependencies = []

operations = [
    {
        "type": "create_table",
        "table": {
            "name": "users",
            "columns": [
                {"name": "id", "type": "INTEGER", "pk": True},
                {"name": "name", "type": "TEXT", "nullable": False},
                {"name": "email", "type": "TEXT", "unique": True},
            ],
            "indexes": [
                {"name": "ix_users_email", "columns": ["email"]},
            ],
        },
    },
]
```

## Supported Operations

### Create Table

```python
{
    "type": "create_table",
    "table": {
        "name": "users",
        "columns": [
            {"name": "id", "type": "INTEGER", "pk": True},
            {"name": "name", "type": "TEXT"},
        ],
    },
}
```

### Drop Table

```python
{
    "type": "drop_table",
    "name": "old_users",
}
```

### Add Column

```python
{
    "type": "add_column",
    "table": "users",
    "field": {
        "name": "age",
        "type": "INTEGER",
        "nullable": True,
    },
}
```

### Drop Column

```python
{
    "type": "drop_column",
    "table": "users",
    "field": "old_field",
}
```

### Alter Column

```python
{
    "type": "alter_column",
    "table": "users",
    "field": "name",
    "changes": {
        "nullable": False,
        "type": "VARCHAR(255)",
    },
}
```

### Add Index

```python
{
    "type": "add_index",
    "table": "users",
    "index": {
        "name": "ix_users_email",
        "columns": ["email"],
        "unique": True,
    },
}
```

### Drop Index

```python
{
    "type": "drop_index",
    "name": "ix_users_old",
}
```

### Add Foreign Key

```python
{
    "type": "add_foreign_key",
    "table": "posts",
    "constraint": {
        "name": "fk_posts_author",
        "column": "author_id",
        "references_table": "users",
        "references_column": "id",
        "on_delete": "CASCADE",
    },
}
```

## Workflow Example

### 1. Define Initial Models

```python
# models.py
from oxyde import OxydeModel, Field

class User(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
```

### 2. Generate Initial Migration

```bash
oxyde makemigrations --name "initial"
```

Creates `migrations/0001_initial.py`.

### 3. Apply Migration

```bash
oxyde migrate
```

### 4. Add New Field

```python
class User(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
    age: int | None = Field(default=None)  # New field
```

### 5. Generate Migration for Change

```bash
oxyde makemigrations --name "add_age"
```

Creates `migrations/0002_add_age.py`.

### 6. Apply New Migration

```bash
oxyde migrate
```

## Database-Specific Considerations

### PostgreSQL

- Full ALTER TABLE support
- Transactional DDL
- Concurrent index creation

### SQLite

- Limited ALTER TABLE (add column only)
- Table recreation for complex changes
- No transactional DDL

### MySQL

- ALTER TABLE with some limitations
- No transactional DDL
- Column changes may require data copy

## Migration Dependencies

Specify dependencies for ordering:

```python
# 0003_add_posts.py

dependencies = ["0002_add_age"]

operations = [
    {
        "type": "create_table",
        "table": {
            "name": "posts",
            "columns": [
                {"name": "id", "type": "INTEGER", "pk": True},
                {"name": "title", "type": "TEXT"},
                {"name": "author_id", "type": "INTEGER"},
            ],
            "foreign_keys": [
                {
                    "column": "author_id",
                    "references_table": "users",
                    "references_column": "id",
                    "on_delete": "CASCADE",
                },
            ],
        },
    },
]
```

## Best Practices

### 1. Review Generated Migrations

Always review generated SQL before applying:

```bash
oxyde sqlmigration myapp 0002
```

### 2. Test on Development First

```bash
# Development
oxyde migrate

# Production (after testing)
oxyde migrate --database production
```

### 3. One Change Per Migration

```bash
# Good
oxyde makemigrations --name "add_user_age"
oxyde makemigrations --name "add_user_bio"

# Avoid: multiple unrelated changes
oxyde makemigrations --name "various_changes"
```

### 4. Don't Edit Applied Migrations

Once a migration is applied to production, create new migrations for fixes.

### 5. Keep Migrations in Version Control

Commit migration files alongside model changes.

## Troubleshooting

### Migration Not Detected

Ensure models are imported before running `makemigrations`:

```python
# In your app's __init__.py
from .models import User, Post, ...
```

### Schema Mismatch

If the database is out of sync:

```bash
# Show current state
oxyde showmigrations

# Fake migration (mark as applied without running)
oxyde migrate --fake 0002
```

### Rollback Failed Migration

```bash
# Rollback to previous state
oxyde migrate 0001

# Or manually fix and re-run
oxyde migrate
```

## Complete Example

```python
# models.py
from datetime import datetime
from oxyde import OxydeModel, Field, Index

class User(OxydeModel):
    class Meta:
        is_table = True
        table_name = "users"

    id: int | None = Field(default=None, db_pk=True)
    email: str = Field(db_unique=True)
    name: str
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")


class Post(OxydeModel):
    class Meta:
        is_table = True
        table_name = "posts"
        indexes = [
            Index(("author_id", "created_at")),
        ]

    id: int | None = Field(default=None, db_pk=True)
    title: str
    content: str
    author: "User" | None = Field(default=None, db_on_delete="CASCADE")
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")
```

```bash
# Generate and apply
oxyde makemigrations --name "initial"
oxyde migrate

# Check status
oxyde showmigrations
# [X] 0001_initial.py
```

## Next Steps

- [Models](models.md) — Model definition
- [Fields](fields.md) — Field options
- [Connections](connections.md) — Database connections

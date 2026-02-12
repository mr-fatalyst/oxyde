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
```

Configuration (migrations directory, dialect) is set in `oxyde_config.py`.

### migrate

Apply pending migrations:

```bash
# Apply all pending
oxyde migrate

# Target specific migration
oxyde migrate 0003_add_posts

# Migrate to "zero" (rollback all)
oxyde migrate zero

# Mark as applied without running (fake)
oxyde migrate 0003_add_posts --fake

# Use specific database alias
oxyde migrate --db-alias analytics
```

### showmigrations

List migration status:

```bash
oxyde showmigrations

# Use specific database alias
oxyde showmigrations --db-alias analytics
```

Output:

```
📋 Migrations status:

  [✓] 0001_initial
  [✓] 0002_add_profile
  [ ] 0003_add_tags

Total: 3 migration(s)
Applied: 2
Pending: 1
```

### sqlmigrate

Show SQL for a migration without running it:

```bash
oxyde sqlmigrate 0001_initial
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
"""Auto-generated migration.

Created: 2024-01-15 10:30:00
"""

depends_on = None


def upgrade(ctx):
    """Apply migration."""
    ctx.create_table(
        "users",
        fields=[
            {"name": "id", "field_type": "INTEGER", "primary_key": True},
            {"name": "name", "field_type": "TEXT", "nullable": False},
            {"name": "email", "field_type": "TEXT", "unique": True},
        ],
        indexes=[
            {"name": "ix_users_email", "columns": ["email"]},
        ],
    )


def downgrade(ctx):
    """Revert migration."""
    ctx.drop_table("users")
```

## Supported Operations

All operations are called on the `ctx` (MigrationContext) object passed to `upgrade()` and `downgrade()`.

### Create Table

```python
ctx.create_table(
    "users",
    fields=[
        {"name": "id", "field_type": "INTEGER", "primary_key": True},
        {"name": "name", "field_type": "TEXT", "nullable": False},
        {"name": "email", "field_type": "TEXT", "unique": True},
    ],
    indexes=[
        {"name": "ix_users_email", "columns": ["email"]},
    ],
)
```

### Drop Table

```python
ctx.drop_table("old_users")
```

### Rename Table

```python
ctx.rename_table("old_name", "new_name")
```

### Add Column

```python
ctx.add_column("users", {
    "name": "age",
    "field_type": "INTEGER",
    "nullable": True,
})
```

### Drop Column

```python
ctx.drop_column("users", "old_field")
```

### Rename Column

```python
ctx.rename_column("users", "old_name", "new_name")
```

### Alter Column

```python
ctx.alter_column("users", "name", nullable=False, type="VARCHAR(255)")
```

### Create Index

```python
ctx.create_index("users", {
    "name": "ix_users_email",
    "columns": ["email"],
    "unique": True,
})
```

### Drop Index

```python
ctx.drop_index("users", "ix_users_old")
```

### Add Foreign Key

```python
ctx.add_foreign_key(
    "posts",
    "fk_posts_author",
    ["author_id"],
    "users",
    ["id"],
    on_delete="CASCADE",
    on_update="NO ACTION",
)
```

### Drop Foreign Key

```python
ctx.drop_foreign_key("posts", "fk_posts_author")
```

### Add Check Constraint

```python
ctx.add_check("users", "chk_age_positive", "age >= 0")
```

### Drop Check Constraint

```python
ctx.drop_check("users", "chk_age_positive")
```

### Execute Raw SQL

For data migrations or unsupported operations:

```python
ctx.execute("UPDATE users SET status = 'active' WHERE status IS NULL")
```

!!! warning "Raw SQL"
    `ctx.execute()` runs arbitrary SQL. Use carefully and ensure it's compatible with your target database.

## Workflow Example

### 1. Define Initial Models

```python
# models.py
from oxyde import Model, Field

class User(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)

    class Meta:
        is_table = True
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
class User(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
    age: int | None = Field(default=None)  # New field

    class Meta:
        is_table = True
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

Dependencies are specified via `depends_on` at the top of the file:

```python
# 0003_add_posts.py
"""Auto-generated migration.

Created: 2024-01-15 11:00:00
"""

depends_on = "0002_add_age"


def upgrade(ctx):
    """Apply migration."""
    ctx.create_table(
        "posts",
        fields=[
            {"name": "id", "field_type": "INTEGER", "primary_key": True},
            {"name": "title", "field_type": "TEXT", "nullable": False},
            {"name": "author_id", "field_type": "INTEGER", "nullable": False},
        ],
    )
    ctx.add_foreign_key(
        "posts",
        "fk_posts_author",
        ["author_id"],
        "users",
        ["id"],
        on_delete="CASCADE",
    )


def downgrade(ctx):
    """Revert migration."""
    ctx.drop_foreign_key("posts", "fk_posts_author")
    ctx.drop_table("posts")
```

## Best Practices

### 1. Review Generated Migrations

Always review generated SQL before applying:

```bash
oxyde sqlmigrate 0002_add_profile
```

### 2. Test on Development First

```bash
# Development
oxyde migrate

# Production (after testing)
oxyde migrate --db-alias production
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
oxyde migrate 0002_add_profile --fake
```

### Rollback Failed Migration

```bash
# Rollback to specific version
oxyde migrate 0001_initial

# Rollback all migrations
oxyde migrate zero
```

## Complete Example

```python
# models.py
from datetime import datetime
from oxyde import Model, Field, Index

class User(Model):
    id: int | None = Field(default=None, db_pk=True)
    email: str = Field(db_unique=True)
    name: str
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")

    class Meta:
        is_table = True
        table_name = "users"


class Post(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    content: str
    author: "User" | None = Field(default=None, db_on_delete="CASCADE")
    created_at: datetime = Field(db_default="CURRENT_TIMESTAMP")

    class Meta:
        is_table = True
        table_name = "posts"
        indexes = [
            Index(("author_id", "created_at")),
        ]
```

```bash
# Generate and apply
oxyde makemigrations --name initial
oxyde migrate

# Check status
oxyde showmigrations
#   [✓] 0001_initial
```

## Next Steps

- [Models](models.md) — Model definition
- [Fields](fields.md) — Field options
- [Connections](connections.md) — Database connections

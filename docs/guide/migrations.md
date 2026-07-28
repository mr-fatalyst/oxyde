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

Operations that cannot be generated automatically produce no SQL; they are printed as a marker instead of silently disappearing:

```
-- manual migration required: enum post_status_enum ['draft', 'published'] -> ['published'] (no automatic SQL; the migration file pairs this with ctx.require_manual(...))
```

### migrations squash

Replace the whole migration history with a single initial migration:

```bash
oxyde migrations squash

# Custom name suffix and no confirmation prompt
oxyde migrations squash --name baseline --yes
```

See [Squashing Migration History](#squashing-migration-history).

## Migration Files

Migrations are Python files in the `migrations/` directory:

```
migrations/
├── 0001_initial.py
├── 0002_add_profile.py
└── 0003_add_tags.py
```

### Migration Structure

Migration files are normally produced by `oxyde makemigrations`; hand-editing them (or writing a data migration by hand) follows the same format:

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
            {
                "name": "id",
                "column_type": {"kind": "big_integer"},
                "nullable": False,
                "primary_key": True,
                "unique": False,
                "auto_increment": True,
            },
            {
                "name": "name",
                "column_type": {"kind": "text"},
                "nullable": False,
                "primary_key": False,
                "unique": False,
            },
            {
                "name": "email",
                "column_type": {"kind": "string", "length": 255},
                "nullable": False,
                "primary_key": False,
                "unique": True,
            },
        ],
        indexes=[
            {"name": "ix_users_email", "fields": ["email"], "unique": False},
        ],
    )


def downgrade(ctx):
    """Revert migration."""
    ctx.drop_table("users")
```

### Field Dicts

Every field dict carries the semantic column type in `column_type` — a tagged
dict whose `kind` decides the DDL type per dialect:

| `kind` | Extra keys | Python type |
|--------|------------|-------------|
| `big_integer` | — | `int` |
| `double` | — | `float` |
| `boolean` | — | `bool` |
| `text` | — | `str` (no `max_length`) |
| `string` | `length` | `str` with `max_length` |
| `blob` | — | `bytes` |
| `date_time`, `date_time_utc`, `date`, `time` | — | `datetime`, `date`, `time` |
| `timedelta` | — | `timedelta` |
| `uuid` | — | `UUID` |
| `decimal` | `precision`, `scale` | `Decimal` |
| `json`, `json_binary` | — | `dict` / `list` |
| `enum` | `name`, `values` | `enum.Enum` subclass |
| `array` | `item` (nested spec) | `list[...]` |

`name`, `column_type`, `nullable`, `primary_key` and `unique` are required;
`default`, `auto_increment` and `db_type` are optional. `db_type` is verbatim
DDL that overrides the type rendered from `column_type` (e.g. `"JSONB"`,
`"CITEXT"`).

Index dicts use `name`, `fields` (column names) and `unique`; `method` and
`where` are optional.

!!! note "Legacy field format"
    Files written before `ColumnTypeSpec` use `"python_type": "int"` instead of
    `column_type`. They still replay, with a `FutureWarning` — see
    [Squashing Migration History](#squashing-migration-history).

## Supported Operations

All operations are called on the `ctx` (MigrationContext) object passed to `upgrade()` and `downgrade()`.

### Create Table

```python
ctx.create_table(
    "users",
    fields=[
        {
            "name": "id",
            "column_type": {"kind": "big_integer"},
            "nullable": False,
            "primary_key": True,
            "unique": False,
            "auto_increment": True,
        },
        {
            "name": "email",
            "column_type": {"kind": "string", "length": 255},
            "nullable": False,
            "primary_key": False,
            "unique": True,
        },
    ],
    indexes=[
        {"name": "ix_users_email", "fields": ["email"], "unique": False},
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
    "column_type": {"kind": "big_integer"},
    "nullable": True,
    "primary_key": False,
    "unique": False,
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
ctx.alter_column(
    "users",
    "name",
    column_type={"kind": "string", "length": 255},
    nullable=False,
)
```

Accepted keyword arguments mirror the optional field keys: `column_type`,
`db_type`, `nullable`, `default`, `unique`, `max_length`, `max_digits`,
`decimal_places`.

### Create Index

```python
ctx.create_index("users", {
    "name": "ix_users_email",
    "fields": ["email"],
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
- Transactional DDL — except migrations containing `ALTER TYPE ... ADD VALUE`, which PostgreSQL forbids inside a transaction (see [Enum Migrations](#enum-migrations))
- Concurrent index creation
- Type changes emit a `USING "col"::text::<type>` clause, so conversions such as text ↔ enum work

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
            {
                "name": "id",
                "column_type": {"kind": "big_integer"},
                "nullable": False,
                "primary_key": True,
                "unique": False,
                "auto_increment": True,
            },
            {
                "name": "title",
                "column_type": {"kind": "text"},
                "nullable": False,
                "primary_key": False,
                "unique": False,
            },
            {
                "name": "author_id",
                "column_type": {"kind": "big_integer"},
                "nullable": False,
                "primary_key": False,
                "unique": False,
            },
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

## Foreign Key Ordering

When `makemigrations` generates `create_table` / `drop_table` operations it topologically sorts tables by their foreign-key graph:

- new tables are emitted so that referenced tables are created before referencing ones;
- dropped tables are emitted in reverse order;
- ties at the same level are broken alphabetically for stable output.

If the schema contains a cyclic FK dependency, `makemigrations` fails with an error listing the tables involved. Break the cycle by making one side of the FK nullable and adding it in a separate migration step (e.g. create the tables first, then `add_foreign_key` afterwards).

## Enum Migrations

Enum fields (see [Fields — Enum Fields](fields.md#enum-fields)) are tracked by `makemigrations`. What happens depends on *how* the value list changed.

### Creating an Enum Type

A new enum type produces `ctx.create_enum_type(...)`, which emits `CREATE TYPE ... AS ENUM (...)` on PostgreSQL and nothing on MySQL/SQLite (the values live in the column definition there):

```python
def upgrade(ctx):
    """Apply migration."""
    ctx.create_enum_type("post_status_enum", ["draft", "published"])
```

### Appending Values — Automatic

Adding values **at the end** of the enum is handled for you — one operation per new value:

```python
def upgrade(ctx):
    """Apply migration."""
    ctx.add_enum_value(
        "post_status_enum",
        "archived",
        fields=[...],   # columns using the type, for the MySQL path
    )
```

| Dialect | Generated SQL |
|---------|---------------|
| PostgreSQL | `ALTER TYPE "post_status_enum" ADD VALUE IF NOT EXISTS 'archived'` |
| MySQL | `ALTER TABLE ... MODIFY COLUMN ...` with the widened `ENUM(...)` for every column using the type |
| SQLite | nothing — the column is plain `TEXT` |

!!! warning "PostgreSQL runs this migration without a transaction"
    PostgreSQL forbids `ALTER TYPE ... ADD VALUE` inside a transaction block. When a migration contains such an operation, Oxyde executes the **entire** migration without a transaction, so a failure halfway through leaves the already-executed statements applied.

    Keep enum value additions in their own migration so the non-transactional window stays as small as possible:

    ```bash
    oxyde makemigrations --name "add_archived_status"
    ```

### Removing or Reordering Values — Manual

Removing a value, renaming it, or changing the declaration order cannot be automated: existing rows may hold the value, and PostgreSQL has no `DROP VALUE`. `makemigrations` still records the change, but pairs it with a guard that fails until you write the SQL:

```python
def upgrade(ctx):
    """Apply migration."""
    ctx.alter_enum_type(
        "post_status_enum",
        old_values=[
            'draft',
            'published',
            'archived',
        ],
        new_values=[
            'draft',
            'published',
        ],
    )
    ctx.require_manual("Manual enum migration required for post_status_enum: ...")
```

- `ctx.alter_enum_type(...)` emits **no SQL** — it only updates the schema state used by migration replay, so later `makemigrations` runs see the new value list.
- `ctx.require_manual(...)` raises at execution time. Replace it with the `ctx.execute(...)` statements that perform the change, and keep `ctx.alter_enum_type(...)`.

A typical hand-written PostgreSQL replacement:

```python
def upgrade(ctx):
    """Apply migration."""
    ctx.alter_enum_type(
        "post_status_enum",
        old_values=['draft', 'published', 'archived'],
        new_values=['draft', 'published'],
    )
    ctx.execute("UPDATE posts SET status = 'draft' WHERE status = 'archived'")
    ctx.execute('ALTER TYPE "post_status_enum" RENAME TO "post_status_enum_old"')
    ctx.execute('CREATE TYPE "post_status_enum" AS ENUM (\'draft\', \'published\')')
    ctx.execute(
        'ALTER TABLE "posts" ALTER COLUMN "status" TYPE "post_status_enum" '
        'USING "status"::text::"post_status_enum"'
    )
    ctx.execute('DROP TYPE "post_status_enum_old"')
```

`oxyde sqlmigrate` prints a `-- manual migration required` marker for these operations instead of an empty preview.

## Squashing Migration History

`oxyde migrations squash` replays the whole history in memory, computes the final schema, and replaces every migration file with a single `0001_<name>.py` written in the current format:

```bash
oxyde migrations squash
```

```
Will squash 14 migration file(s) into one:
  • 0001_initial.py
  ...

Delete these files and write a new 0001 migration? [y/N]: y
✅ Created 0001_squashed.py (9 table(s)), deleted 14 file(s).
```

What to know before running it:

- **Old files are deleted.** Version control is the backup. The new file is rendered into a temporary directory first, so a generation failure cannot lose the history.
- **Raw SQL is not carried over.** `ctx.execute()` statements are schema-neutral and are dropped; the affected file names are printed so you can move the logic manually.
- **Already-deployed databases** must record the new initial migration without executing it:

    ```bash
    oxyde migrate --fake      # fresh databases: just `oxyde migrate`
    ```

    Tracker records of the old migration names are harmless — pending migrations are computed as files minus applied.

!!! note "Legacy migration format"
    Migration files written before the `ColumnTypeSpec` format (field dicts with `python_type`) still replay, but emit a `FutureWarning` pointing to this command. Support for the legacy format is removed in 1.0 — squash converts them.

## Programmatic Schema Management

For tests and scripts where migration files are not needed, use `create_tables()` / `drop_tables()`:

```python
from oxyde import AsyncDatabase, create_tables, drop_tables

database = AsyncDatabase("sqlite:///:memory:", name="default")
async with database:
    await create_tables(database)
    # ... run tests ...
    await drop_tables(database)
```

See [Connections — Schema Management](connections.md#schema-management) for details.

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
    author: User | None = Field(default=None, db_on_delete="CASCADE")
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

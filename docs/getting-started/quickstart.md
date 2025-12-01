# Quick Start

This guide will get you up and running with Oxyde in 5 minutes.

## Define a Model

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

Key points:

- Inherit from `OxydeModel`
- Set `is_table = True` in `Meta` class
- Use `Field()` for database metadata
- `db_pk=True` marks the primary key
- `db_unique=True` creates a UNIQUE constraint

## Connect to Database

```python
from oxyde import db

# Initialize connection
await db.init(default="sqlite:///app.db")

# ... your code ...

# Close connection
await db.close()
```

Or use a context manager:

```python
async with db.connect("sqlite:///app.db"):
    # Connection is open here
    pass
# Connection is closed automatically
```

## CRUD Operations

### Create

```python
# Single record
user = await User.objects.create(
    name="Alice",
    email="alice@example.com",
    age=30
)
print(user.id)  # Auto-generated ID

# Or create instance and save
user = User(name="Bob", email="bob@example.com")
await user.save()
```

### Read

```python
# Get all
users = await User.objects.all()

# Get by ID (raises NotFoundError if not found)
user = await User.objects.get(id=1)

# Get or None
user = await User.objects.get_or_none(id=999)

# Filter
adults = await User.objects.filter(age__gte=18).all()

# First/Last
first_user = await User.objects.first()
last_user = await User.objects.last()
```

### Update

```python
# Update via instance
user = await User.objects.get(id=1)
user.name = "Alice Smith"
await user.save()

# Bulk update
count = await User.objects.filter(age__lt=18).update(status="minor")
```

### Delete

```python
# Delete via instance
user = await User.objects.get(id=1)
await user.delete()

# Bulk delete
count = await User.objects.filter(status="inactive").delete()
```

## Complete Example

```python
import asyncio
from oxyde import OxydeModel, Field, db

class User(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
    age: int | None = Field(default=None)

async def main():
    # Connect to SQLite
    async with db.connect("sqlite:///app.db"):
        # Create users
        alice = await User.objects.create(
            name="Alice", email="alice@example.com", age=30
        )
        bob = await User.objects.create(
            name="Bob", email="bob@example.com", age=25
        )

        # Query
        users = await User.objects.filter(age__gte=25).all()
        print(f"Found {len(users)} users aged 25+")

        # Update
        alice.age = 31
        await alice.save()

        # Delete
        await bob.delete()

        # Count
        count = await User.objects.count()
        print(f"Total users: {count}")

if __name__ == "__main__":
    asyncio.run(main())
```

## Next Steps

- [First Project](first-project.md) — Build a complete application
- [Models](../guide/models.md) — Learn model definition in depth
- [Queries](../guide/queries.md) — Master the QuerySet API

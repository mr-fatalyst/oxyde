# Frequently Asked Questions

Common questions and answers about Oxyde.

## General

### What is Oxyde?

Oxyde is a high-performance async Python ORM with a Rust core. It combines:

- **Python API**: Pydantic v2 models with Django-style query syntax
- **Rust Core**: High-performance SQL generation and execution via sqlx
- **MessagePack Protocol**: Efficient binary communication (~2KB payloads)

### Why Rust?

The Rust core provides:

- **Performance**: SQL generation and connection pooling are 5-10x faster than pure Python
- **Memory Safety**: No GC pauses during I/O operations
- **Concurrency**: True async I/O that releases Python's GIL

### Which databases are supported?

| Database | Status | Features |
|----------|--------|----------|
| PostgreSQL | Full support | RETURNING, JSONB, Arrays, UPSERT |
| SQLite | Full support | RETURNING, WAL mode, connection pooling |
| MySQL | Full support | No RETURNING (uses LAST_INSERT_ID), UPSERT via ON DUPLICATE KEY |

### Is Oxyde production-ready?

Oxyde is suitable for production use with these considerations:

- Core CRUD operations are stable
- Advanced features (migrations, M2M) are still maturing
- Thoroughly test your specific use case

## Installation

### How do I install Oxyde?

```bash
pip install oxyde
```

### What are the system requirements?

- Python 3.10+
- No Rust compiler needed (wheels are pre-built)

### How do I install from source?

```bash
# Clone repository
git clone https://github.com/mr-fatalyst/oxyde.git
cd oxyde

# Install Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build and install
cd crates/oxyde-core-py
maturin develop --release
cd ../../python
pip install -e .
```

## Models

### How do I define a model?

```python
from oxyde import OxydeModel, Field

class User(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str = Field(db_unique=True)
```

### Why isn't my model creating a table?

Ensure you have `is_table = True` in the Meta class:

```python
class User(OxydeModel):
    class Meta:
        is_table = True  # Required!
```

### How do I use UUIDs as primary keys?

```python
from uuid import UUID

class User(OxydeModel):
    class Meta:
        is_table = True

    id: UUID = Field(
        db_pk=True,
        db_default="gen_random_uuid()"  # PostgreSQL
    )
```

### How do I add Pydantic validation?

Oxyde fields support all Pydantic validation:

```python
class User(OxydeModel):
    age: int = Field(ge=0, le=150)
    email: str = Field(pattern=r"^[\w.-]+@[\w.-]+\.\w+$")
    name: str = Field(min_length=1, max_length=100)
```

## Queries

### How do I filter with OR conditions?

Use Q expressions:

```python
from oxyde import Q

users = await User.objects.filter(
    Q(status="active") | Q(premium=True)
).all()
```

### How do I do case-insensitive search?

Use `__icontains`, `__iexact`, etc.:

```python
users = await User.objects.filter(name__icontains="alice").all()
users = await User.objects.filter(email__iexact="ALICE@EXAMPLE.COM").all()
```

### How do I paginate results?

```python
# Page 1 (first 20)
page1 = await User.objects.limit(20).all()

# Page 2
page2 = await User.objects.offset(20).limit(20).all()

# With ordering
users = await User.objects.order_by("-created_at").limit(20).offset(40).all()
```

### How do I count rows efficiently?

```python
# Don't do this (loads all rows)
count = len(await User.objects.all())

# Do this instead
count = await User.objects.filter(status="active").count()
```

### How do I load related objects?

Use `join()` for FK relations:

```python
# Without join - author not loaded
posts = await Post.objects.all()
for post in posts:
    print(post.author)  # None - not loaded

# With join - author loaded in same query
posts = await Post.objects.join("author").all()
for post in posts:
    print(post.author.name)  # Data available
```

### How do I do atomic updates?

Use F expressions:

```python
from oxyde import F

# Atomic increment
await Post.objects.filter(id=1).update(views=F("views") + 1)

# Atomic decrement
await User.objects.filter(id=1).update(balance=F("balance") - 100)
```

## Connections

### How do I connect to the database?

```python
from oxyde import db

await db.init("postgresql://user:pass@localhost/mydb")
```

### How do I configure connection pooling?

```python
from oxyde import db, PoolSettings

await db.init(
    "postgresql://localhost/mydb",
    settings=PoolSettings(
        max_connections=20,
        min_connections=5,
        acquire_timeout=30,
    )
)
```

### How do I use multiple databases?

```python
await db.init(
    default="postgresql://localhost/main",
    analytics="postgresql://localhost/analytics",
)

# Use specific database
events = await Event.objects.all(using="analytics")
```

### How do I close connections properly?

```python
from oxyde import db

# In application shutdown
await db.close()

# With FastAPI lifespan
app = FastAPI(lifespan=db.lifespan("postgresql://localhost/mydb"))
```

## Transactions

### How do I use transactions?

```python
from oxyde.db import transaction

async with transaction.atomic():
    user = await User.objects.create(name="Alice")
    await Account.objects.create(user_id=user.id, balance=0)
```

### What happens on exception?

The transaction is automatically rolled back:

```python
async with transaction.atomic():
    await User.objects.create(name="Alice")
    raise ValueError("Oops!")  # Transaction rolled back
```

### How do I use savepoints?

Nest `transaction.atomic()` blocks:

```python
async with transaction.atomic():
    await User.objects.create(name="Alice")

    try:
        async with transaction.atomic():
            await User.objects.create(name="Bob")
            raise ValueError()  # Inner transaction rolled back
    except ValueError:
        pass  # Alice is still committed

    await User.objects.create(name="Charlie")
```

### How do I lock rows?

Use `for_update()` or `for_share()`:

```python
async with atomic():
    user = await User.objects.filter(id=1).for_update().first()
    user.balance -= 100
    await user.save()
```

## Performance

### Why are my queries slow?

Common causes:

1. **Missing indexes**: Add `db_index=True` to filtered fields
2. **Queries in loops**: Use `join()` or `prefetch()` to load related objects
3. **Large result sets**: Use `limit()` and pagination
4. **Missing connection pool**: Configure `min_connections > 0`

### How do I analyze query performance?

```python
# Get query plan
plan = await User.objects.filter(age__gte=18).explain()
print(plan)

# With execution times
plan = await User.objects.filter(age__gte=18).explain(analyze=True)
```

### How do I see the generated SQL?

```python
sql, params = User.objects.filter(age__gte=18).sql()
print(sql)  # SELECT ... WHERE age >= $1
print(params)  # [18]
```

### What are the best SQLite settings?

Oxyde applies optimized defaults automatically:

- WAL journal mode (10-20x faster writes)
- NORMAL synchronous mode
- 10MB cache size
- 5 second busy timeout

## Errors

### NotFoundError

Raised when `get()` finds no rows:

```python
try:
    user = await User.objects.get(id=999)
except NotFoundError:
    print("User not found")

# Or use get_or_none
user = await User.objects.get_or_none(id=999)
```

### MultipleObjectsReturned

Raised when `get()` finds multiple rows:

```python
# Wrong - status is not unique
user = await User.objects.get(status="active")

# Correct - use unique field
user = await User.objects.get(email="alice@example.com")

# Or use first() for non-unique queries
user = await User.objects.filter(status="active").first()
```

### IntegrityError

Raised on constraint violations:

```python
try:
    await User.objects.create(email="duplicate@example.com")
except IntegrityError:
    print("Email already exists")

# Better: use get_or_create
user, created = await User.objects.get_or_create(
    email="alice@example.com",
    defaults={"name": "Alice"}
)
```

## Migrations

### How do I create migrations?

```bash
oxyde makemigrations
```

### How do I apply migrations?

```bash
oxyde migrate
```

### How do I see migration status?

```bash
oxyde showmigrations
```

### How do I rollback a migration?

```bash
oxyde migrate 0001  # Rollback to migration 0001
```

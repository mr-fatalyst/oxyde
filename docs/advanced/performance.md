# Performance

Oxyde is designed for high performance through its Rust core. This guide covers optimization techniques.

## Architecture Overview

```
Python Layer                Rust Core                Database
┌─────────────┐          ┌─────────────┐           ┌───────────┐
│ Pydantic    │──────────│ SQL Gen     │───────────│ PostgreSQL│
│ Models      │ msgpack  │ Connection  │    sqlx   │ SQLite    │
│ QuerySet    │  ~2KB    │ Pool        │           │ MySQL     │
└─────────────┘          └─────────────┘           └───────────┘
```

Key performance characteristics:

- **MessagePack protocol**: ~2KB binary payloads
- **Rust SQL generation**: sea-query for fast SQL building
- **Native async**: sqlx releases Python GIL during I/O
- **Connection pooling**: Efficient connection reuse

## Query Optimization

### Select Only Needed Fields

```python
# BAD: Loads all columns
users = await User.objects.all()

# GOOD: Load only what you need
users = await User.objects.values("id", "name").all()
```

### Use Limits

```python
# BAD: Loads all records
users = await User.objects.filter(status="active").all()

# GOOD: Paginate
users = await User.objects.filter(status="active").limit(100).all()
```

### Batch Operations

```python
# BAD: N individual inserts
for item in items:
    await Item.objects.create(**item)

# GOOD: Bulk insert
await Item.objects.bulk_create([Item(**item) for item in items])
```

### Use F Expressions

```python
# BAD: Read-modify-write (race condition + 2 queries)
post = await Post.objects.get(id=1)
post.views += 1
await post.save()

# GOOD: Atomic update (1 query)
await Post.objects.filter(id=1).update(views=F("views") + 1)
```

### Avoid Querying in Loops

```python
# BAD: Multiple queries
posts = await Post.objects.all()
for post in posts:
    author = await Author.objects.get(id=post.author_id)  # N queries!

# GOOD: Eager loading with join (1 query)
posts = await Post.objects.join("author").all()
for post in posts:
    print(post.author.name)  # Data already loaded
```

## Connection Pool Tuning

### Pool Size

```python
from oxyde import PoolSettings

# Web API: many concurrent connections
settings = PoolSettings(
    max_connections=50,
    min_connections=10,
)

# Background worker: fewer long-lived connections
settings = PoolSettings(
    max_connections=10,
    min_connections=2,
)

# SQLite: limited concurrency
settings = PoolSettings(
    max_connections=5,  # SQLite handles this well with WAL
)
```

### Connection Lifecycle

```python
settings = PoolSettings(
    acquire_timeout=30,     # Don't wait forever
    idle_timeout=300,       # Close idle after 5 min
    max_lifetime=3600,      # Refresh connections hourly
    test_before_acquire=True,  # Ping before use
)
```

## SQLite Optimization

Oxyde applies optimized defaults for SQLite:

```python
# Default settings (applied automatically)
PoolSettings(
    sqlite_journal_mode="WAL",    # 10-20x faster writes
    sqlite_synchronous="NORMAL",  # Balance safety/speed
    sqlite_cache_size=10000,      # ~10MB cache
    sqlite_busy_timeout=5000,     # 5 sec lock timeout
)
```

### WAL Mode Benefits

- **Concurrent reads**: Multiple readers don't block
- **Faster writes**: Sequential log instead of random I/O
- **Crash recovery**: Better durability

### When to Override

```python
# Maximum safety (slower)
PoolSettings(
    sqlite_journal_mode="DELETE",
    sqlite_synchronous="FULL",
)

# Maximum speed (less safe for power failure)
PoolSettings(
    sqlite_synchronous="OFF",
    sqlite_cache_size=50000,  # 50MB cache
)
```

## Indexing Strategies

### Single-Column Indexes

```python
class User(Model):
    email: str = Field(db_index=True)  # For equality lookups
    created_at: datetime = Field(db_index=True)  # For range queries

    class Meta:
        is_table = True
```

### Composite Indexes

```python
from oxyde import Index

class Event(Model):
    user_id: int
    created_at: datetime

    class Meta:
        is_table = True
        indexes = [
            # Order matters: (user_id, date) supports:
            # - WHERE user_id = ?
            # - WHERE user_id = ? AND date > ?
            # But NOT: WHERE date > ? (without user_id)
            Index(("user_id", "created_at")),
        ]
```

### Partial Indexes

```python
class User(Model):
    email: str
    deleted_at: datetime | None = None

    class Meta:
        is_table = True
        indexes = [
            # Only index active users
            Index(("email",), unique=True, where="deleted_at IS NULL"),
        ]
```

## Explain Queries

Analyze query performance:

```python
# Get query plan
plan = await User.objects.filter(age__gte=18).explain()
print(plan)

# With execution times
plan = await User.objects.filter(age__gte=18).explain(analyze=True)
print(plan)
```

PostgreSQL output example:

```
Seq Scan on users  (cost=0.00..1.50 rows=33 width=40)
  Filter: (age >= 18)
```

Add index if you see "Seq Scan" on large tables.

## Async Concurrency

### Concurrent Queries

```python
import asyncio

# BAD: Sequential
user = await User.objects.get(id=1)
posts = await Post.objects.filter(author_id=1).all()
comments = await Comment.objects.filter(user_id=1).all()

# GOOD: Concurrent
user, posts, comments = await asyncio.gather(
    User.objects.get(id=1),
    Post.objects.filter(author_id=1).all(),
    Comment.objects.filter(user_id=1).all(),
)
```

### Task Groups (Python 3.11+)

```python
async with asyncio.TaskGroup() as tg:
    user_task = tg.create_task(User.objects.get(id=1))
    posts_task = tg.create_task(Post.objects.filter(author_id=1).all())

user = user_task.result()
posts = posts_task.result()
```

## Benchmarking Tips

### Warm Up

```python
# Warm up connection pool
await User.objects.first()

# Then benchmark
import time
start = time.perf_counter()
for _ in range(1000):
    await User.objects.filter(status="active").all()
elapsed = time.perf_counter() - start
print(f"1000 queries in {elapsed:.2f}s ({1000/elapsed:.0f} qps)")
```

### Realistic Conditions

- Use production-like data volumes
- Test with concurrent connections
- Include network latency

## Common Bottlenecks

| Symptom | Cause | Solution |
|---------|-------|----------|
| Slow queries | Missing index | Add index, use explain() |
| High latency | Queries in loops | Use join() or prefetch() |
| Pool exhaustion | Too few connections | Increase max_connections |
| Lock contention | Long transactions | Shorten transactions |
| Memory spikes | Large result sets | Use limit(), pagination |

## Performance Checklist

- [ ] Use `values()` to select only needed columns
- [ ] Add indexes for filtered columns
- [ ] Use `join()` to load related objects
- [ ] Use `bulk_create()` for batch inserts
- [ ] Use `F()` expressions for atomic updates
- [ ] Configure appropriate pool size
- [ ] Use SQLite WAL mode (default)
- [ ] Profile slow queries with `explain()`
- [ ] Use `asyncio.gather()` for concurrent queries

## Next Steps

- [Internals](internals.md) — Rust core architecture
- [Connections](../guide/connections.md) — Connection configuration

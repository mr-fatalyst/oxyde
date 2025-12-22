# Database Connections

Oxyde provides multiple ways to manage database connections.

## Quick Start

```python
from oxyde import db

# Initialize
await db.init(default="postgresql://localhost/mydb")

# Use models
users = await User.objects.all()

# Close
await db.close()
```

## Connection URLs

| Database | URL Format |
|----------|------------|
| PostgreSQL | `postgresql://user:pass@host:5432/database` |
| PostgreSQL | `postgres://user:pass@host:5432/database` |
| SQLite (file) | `sqlite:///path/to/file.db` |
| SQLite (memory) | `sqlite:///:memory:` |
| MySQL | `mysql://user:pass@host:3306/database` |

!!! note "SQLite File Paths"
    SQLite relative paths (like `sqlite:///app.db`) are resolved from the **current working directory**, not from the config file location. For portable projects, use absolute paths:

    ```python
    # oxyde_config.py
    from pathlib import Path

    BASE_DIR = Path(__file__).parent

    DATABASES = {
        "default": f"sqlite:///{BASE_DIR}/app.db"
    }
    ```

## High-Level API

### db.init()

Initialize one or more database connections:

```python
from oxyde import db, PoolSettings

# Single database
await db.init(default="postgresql://localhost/mydb")

# Multiple databases
await db.init(
    default="postgresql://localhost/main",
    analytics="postgresql://localhost/analytics",
    cache="sqlite:///:memory:",
)

# With custom settings
await db.init(
    default="postgresql://localhost/mydb",
    settings=PoolSettings(max_connections=20),
)
```

### db.close()

Close all connections gracefully:

```python
await db.close()
```

This rolls back any active transactions before closing.

### db.connect()

Context manager for scripts and tests:

```python
async with db.connect("sqlite:///:memory:") as conn:
    users = await User.objects.all()
# Connection closed automatically
```

With custom name:

```python
async with db.connect("sqlite:///test.db", name="test") as conn:
    users = await User.objects.all(using="test")
```

### db.lifespan()

FastAPI integration:

```python
from fastapi import FastAPI
from oxyde import db

app = FastAPI(
    lifespan=db.lifespan(
        default="postgresql://localhost/mydb",
        settings=PoolSettings(max_connections=50),
    )
)

@app.get("/users")
async def get_users():
    return await User.objects.all()
```

## AsyncDatabase

Low-level connection wrapper:

```python
from oxyde import AsyncDatabase, PoolSettings

database = AsyncDatabase(
    "postgresql://localhost/mydb",
    name="default",
    settings=PoolSettings(max_connections=20),
)

# Manual lifecycle
await database.connect()
# ... use database ...
await database.disconnect()

# Or as context manager
async with database:
    users = await User.objects.all()
```

## Pool Settings

Configure connection pool behavior:

```python
from oxyde import PoolSettings
from datetime import timedelta

settings = PoolSettings(
    # Pool size
    max_connections=20,           # Maximum pool size
    min_connections=5,            # Minimum idle connections

    # Timeouts
    acquire_timeout=30.0,         # Max wait for connection (seconds)
    idle_timeout=600.0,           # Close idle connections after (seconds)
    max_lifetime=1800.0,          # Max connection age (seconds)

    # Health check
    test_before_acquire=True,     # Ping before using connection

    # Transaction cleanup (background task)
    transaction_timeout=300,      # Max transaction age (seconds)
    transaction_cleanup_interval=60,  # Cleanup check interval (seconds)
)
```

### SQLite Settings

SQLite-specific PRAGMA settings (applied automatically):

```python
settings = PoolSettings(
    # WAL mode for better concurrent writes (10-20x faster)
    sqlite_journal_mode="WAL",

    # Balance between speed and safety
    sqlite_synchronous="NORMAL",

    # Cache size in pages (~10MB)
    sqlite_cache_size=10000,

    # Lock timeout in milliseconds
    sqlite_busy_timeout=5000,
)
```

Default settings are optimized for most use cases.

## Multiple Databases

### Configuration

```python
await db.init(
    default="postgresql://localhost/main",
    analytics="postgresql://localhost/analytics",
    legacy="mysql://localhost/old_system",
)
```

### Using Specific Database

```python
# Default database
users = await User.objects.all()

# Specific database
events = await Event.objects.all(using="analytics")
old_users = await LegacyUser.objects.all(using="legacy")
```

### Transactions Across Databases

Each database has separate transactions:

```python
from oxyde.db import transaction

# Transaction on default database
async with transaction.atomic():
    await User.objects.create(name="Alice")

# Transaction on analytics database
async with transaction.atomic(using="analytics"):
    await Event.objects.create(type="signup")
```

## Connection Registry

### Get Connection by Name

```python
from oxyde import get_connection

conn = await get_connection("default")
print(conn.connected)  # True
```

### Register Custom Connection

```python
from oxyde import AsyncDatabase, register_connection

database = AsyncDatabase("postgresql://localhost/custom", name="custom")
register_connection(database)
await database.connect()

# Now available as "custom"
users = await User.objects.all(using="custom")
```

### Disconnect All

```python
from oxyde import disconnect_all

await disconnect_all()  # Close all registered connections
```

## Error Handling

```python
from oxyde import db
from oxyde.exceptions import ManagerError

try:
    await db.init(default="postgresql://invalid-host/db")
except Exception as e:
    print(f"Connection failed: {e}")
```

## Best Practices

### 1. Use Context Managers for Scripts

```python
async def main():
    async with db.connect("sqlite:///app.db"):
        # Automatic cleanup on exit
        await run_app()
```

### 2. Use Lifespan for Web Apps

```python
app = FastAPI(lifespan=db.lifespan(default="postgresql://..."))
```

### 3. Configure Pool Size Based on Workload

```python
# Web API: many short connections
PoolSettings(max_connections=50, min_connections=10)

# Background worker: fewer long connections
PoolSettings(max_connections=10, min_connections=2)

# SQLite: single connection is usually enough
PoolSettings(max_connections=1)
```

### 4. Set Appropriate Timeouts

```python
# Production
PoolSettings(
    acquire_timeout=30,      # Don't wait forever
    idle_timeout=300,        # Close idle after 5 min
    max_lifetime=3600,       # Refresh connections hourly
)
```

## Advanced: Multiple Databases

### Per-Database Settings

```python
from oxyde import AsyncDatabase, PoolSettings

# Main database: high concurrency
main_db = AsyncDatabase(
    "postgresql://localhost/main",
    name="default",
    settings=PoolSettings(
        max_connections=50,
        min_connections=10,
    ),
)

# Analytics: read-heavy, fewer connections
analytics_db = AsyncDatabase(
    "postgresql://localhost/analytics",
    name="analytics",
    settings=PoolSettings(
        max_connections=10,
        min_connections=2,
    ),
)

await main_db.connect()
await analytics_db.connect()
```

### Read Replicas

```python
await db.init(
    default="postgresql://primary/db",
    replica="postgresql://replica/db",
)

async def get_user(user_id: int):
    # Read from replica
    return await User.objects.get(id=user_id, using="replica")

async def update_user(user_id: int, **data):
    # Write to primary
    await User.objects.filter(id=user_id).update(**data)
```

### Cross-Database Operations

```python
async def sync_user_to_analytics(user_id: int):
    # Read from main
    user = await User.objects.get(id=user_id)

    # Write to analytics
    await UserProfile.objects.create(
        user_id=user.id,
        name=user.name,
        using="analytics"
    )
```

### Dynamic Tenant Connections

```python
from oxyde import AsyncDatabase

class TenantConnectionPool:
    def __init__(self):
        self._connections: dict[int, AsyncDatabase] = {}

    async def get_connection(self, tenant_id: int) -> AsyncDatabase:
        if tenant_id not in self._connections:
            tenant = await Tenant.objects.get(id=tenant_id)
            conn = AsyncDatabase(
                tenant.database_url,
                name=f"tenant_{tenant_id}",
            )
            await conn.connect()
            self._connections[tenant_id] = conn
        return self._connections[tenant_id]

    async def close_all(self):
        for conn in self._connections.values():
            await conn.disconnect()
        self._connections.clear()

tenant_pool = TenantConnectionPool()
```

### FastAPI with Multiple Databases

```python
from fastapi import FastAPI, Depends, Request
from oxyde import db, PoolSettings

app = FastAPI(
    lifespan=db.lifespan(
        default="postgresql://localhost/main",
        analytics="postgresql://localhost/analytics",
        settings=PoolSettings(max_connections=20),
    )
)

@app.get("/users")
async def get_users():
    return await User.objects.all()

@app.get("/events")
async def get_events():
    return await Event.objects.all(using="analytics")
```

### Testing with Multiple Databases

```python
import pytest

@pytest.fixture
async def test_dbs():
    await db.init(
        default="sqlite:///:memory:",
        analytics="sqlite:///:memory:",
    )
    yield
    await db.close()

@pytest.mark.asyncio
async def test_cross_db(test_dbs):
    user = await User.objects.create(name="Test")
    await Event.objects.create(
        type="test",
        user_id=user.id,
        using="analytics"
    )

    events = await Event.objects.filter(user_id=user.id).all(using="analytics")
    assert len(events) == 1
```

## Next Steps

- [Queries](queries.md) — Query your models
- [Transactions](transactions.md) — Transaction handling

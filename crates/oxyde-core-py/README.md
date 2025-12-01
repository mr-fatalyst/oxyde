# Oxyde-core

Native Rust backend for [Oxyde ORM](https://github.com/mr-fatalyst/oxyde). Provides high-performance database operations via PyO3 bindings.

## Core Technologies

- **[PyO3](https://pyo3.rs)** — Rust bindings for Python, async support via `pyo3-asyncio`
- **[sqlx](https://github.com/launchbadge/sqlx)** — Async SQL toolkit with compile-time checked queries
- **[sea-query](https://github.com/SeaQL/sea-query)** — Dynamic SQL query builder for multiple backends
- **[MessagePack](https://msgpack.org)** — Binary serialization (~2KB per query), faster than JSON
- **[tokio](https://tokio.rs)** — Async runtime, GIL released during I/O operations

## Why Rust?

1. **Performance** — Native code execution, zero-copy where possible
2. **True async** — GIL released during database I/O, enabling real parallelism
3. **Memory safety** — No segfaults, no data races
4. **Connection pooling** — Efficient pool management with sqlx

## Architecture

`oxyde-core` is a Python extension module (`.so`/`.pyd`) built with PyO3. It exposes async functions that handle the entire database pipeline:

```
Python (oxyde)                    Rust (oxyde-core)
     │                                  │
     │  QueryIR (MessagePack)           │
     ├─────────────────────────────────►│
     │                                  ├── oxyde-codec: Deserialize IR
     │                                  ├── oxyde-query: Generate SQL (sea_query)
     │                                  ├── oxyde-driver: Execute (sqlx pools)
     │                                  │
     │  Results (MessagePack)           │
     │◄─────────────────────────────────┤
     │                                  │
```

## Internal Crates

| Crate | Purpose |
|-------|---------|
| `oxyde-codec` | MessagePack IR protocol, query structure validation |
| `oxyde-query` | IR → SQL conversion using sea_query |
| `oxyde-driver` | Connection pools (sqlx), query execution, transactions |
| `oxyde-migrate` | Schema diff, migration SQL generation |

## Exposed Functions

### Connection Management
- `init_pool(name, url, settings)` — Initialize named connection pool
- `init_pool_overwrite(name, url, settings)` — Replace existing pool
- `close_pool(name)` — Close specific pool
- `close_all_pools()` — Close all pools

### Query Execution
- `execute(pool_name, ir_bytes)` — Execute query, return MessagePack results
- `execute_in_transaction(pool_name, tx_id, ir_bytes)` — Execute within transaction
- `render_sql(pool_name, ir_bytes)` — Get SQL and params without executing
- `render_sql_debug(ir_bytes, dialect)` — Render SQL for specific dialect
- `explain(pool_name, ir_bytes, analyze, format)` — Get query execution plan

### Transactions
- `begin_transaction(pool_name)` — Start transaction, return tx_id
- `commit_transaction(tx_id)` — Commit transaction
- `rollback_transaction(tx_id)` — Rollback transaction
- `create_savepoint(tx_id, name)` — Create savepoint
- `rollback_to_savepoint(tx_id, name)` — Rollback to savepoint
- `release_savepoint(tx_id, name)` — Release savepoint

### Migrations
- `migration_compute_diff(old_json, new_json)` — Compute schema diff
- `migration_to_sql(operations_json, dialect)` — Generate migration SQL

## Supported Databases

- **PostgreSQL** — Full support (RETURNING, UPSERT, JSON, arrays)
- **SQLite** — Full support (WAL mode, RETURNING)
- **MySQL** — Full support (auto-increment IDs via `last_insert_id()`, UPSERT via ON DUPLICATE KEY)

## Installation

This package is installed automatically as a dependency of `oxyde`:

```bash
pip install oxyde
```

For development:

```bash
cd crates/oxyde-core-py
maturin develop --release
```

## Usage

Import from `oxyde`, not directly from `oxyde-core`:

```python
from oxyde import OxydeModel, Field, db

class User(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    email: str = Field(db_unique=True)

await db.init(default="postgresql://localhost/mydb")
users = await User.objects.all()
```

## License

MIT

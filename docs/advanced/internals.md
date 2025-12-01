# Internals (Rust Core)

This section documents Oxyde's Rust architecture for advanced users and contributors.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Python Layer                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
│  │ OxydeModel  │  │   Query     │  │    db.*     │              │
│  │  (Pydantic) │  │  (Builder)  │  │ (Async API) │              │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘              │
│         │                │                │                      │
│         └────────────────┼────────────────┘                      │
│                          │                                       │
│                    MessagePack (~2KB)                            │
│                          │                                       │
└──────────────────────────┼───────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────────┐
│                        Rust Core                                  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
│  │ oxyde-codec │  │ oxyde-query │  │oxyde-driver │              │
│  │  (IR Parse) │  │ (SQL Gen)   │  │(Connection) │              │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘              │
│         │                │                │                      │
│         └────────────────┼────────────────┘                      │
│                          │                                       │
│                       sqlx                                       │
│                          │                                       │
└──────────────────────────┼───────────────────────────────────────┘
                           │
                           ▼
              ┌─────────────────────────┐
              │   Database              │
              │  PostgreSQL/SQLite/MySQL│
              └─────────────────────────┘
```

## Rust Crates

### oxyde-codec

**Purpose**: Define and validate the IR (Intermediate Representation) protocol.

**Location**: `crates/oxyde-codec/src/lib.rs`

Key types:

```rust
pub struct QueryIR {
    pub operation: Operation,
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub filter_tree: Option<FilterNode>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub order_by: Option<Vec<(String, String)>>,
    // ... more fields
}

pub enum Operation {
    Select,
    Insert,
    Update,
    Delete,
}

pub enum FilterNode {
    Condition {
        field: String,
        op: String,
        value: Value,
    },
    And(Vec<FilterNode>),
    Or(Vec<FilterNode>),
    Not(Box<FilterNode>),
}
```

### oxyde-query

**Purpose**: Generate SQL from IR using sea-query.

**Location**: `crates/oxyde-query/src/lib.rs`

Key functions:

```rust
pub fn build_sql(ir: &QueryIR, dialect: Dialect) -> Result<(String, Vec<Value>)> {
    match ir.operation {
        Operation::Select => build_select(ir, dialect),
        Operation::Insert => build_insert(ir, dialect),
        Operation::Update => build_update(ir, dialect),
        Operation::Delete => build_delete(ir, dialect),
    }
}

pub enum Dialect {
    Postgres,
    Sqlite,
    Mysql,
}
```

### oxyde-driver

**Purpose**: Connection pooling, query execution, transaction management.

**Location**: `crates/oxyde-driver/src/lib.rs`

Key components:

```rust
// Connection pool registry (global)
static POOL_REGISTRY: OnceCell<ConnectionRegistry> = OnceCell::const_new();

// Pool handle for each connection
pub struct PoolHandle {
    pub pool: DbPool,
    pub backend: DatabaseBackend,
}

pub enum DbPool {
    Postgres(PgPool),
    MySql(MySqlPool),
    Sqlite(SqlitePool),
}

// Transaction management
pub struct TransactionInner {
    pub conn: Option<DbConn>,
    pub state: TransactionState,
    pub created_at: Instant,
}
```

### oxyde-migrate

**Purpose**: Schema diffing and migration generation.

**Location**: `crates/oxyde-migrate/src/lib.rs`

Key functions:

```rust
pub fn compute_diff(old_schema: &Schema, new_schema: &Schema) -> Vec<Operation> {
    // Compare tables, columns, indexes
    // Generate add/drop/alter operations
}

pub fn generate_migration(operations: &[Operation], dialect: Dialect) -> String {
    // Generate SQL migration script
}
```

### oxyde-core-py

**Purpose**: PyO3 bindings exposing Rust functions to Python.

**Location**: `crates/oxyde-core-py/src/lib.rs`

Exposed functions:

```rust
#[pyfunction]
fn init_pool(py: Python, name: String, url: String, settings: Option<HashMap<String, Value>>) -> PyResult<&PyAny> {
    pyo3_asyncio::tokio::future_into_py(py, async move {
        // Initialize pool in registry
    })
}

#[pyfunction]
fn execute(py: Python, pool_name: String, ir_bytes: &[u8]) -> PyResult<&PyAny> {
    pyo3_asyncio::tokio::future_into_py(py, async move {
        // 1. Deserialize IR from MessagePack
        // 2. Build SQL
        // 3. Execute via sqlx
        // 4. Serialize results to MessagePack
    })
}

#[pyfunction]
fn begin_transaction(py: Python, pool_name: String) -> PyResult<&PyAny> { ... }

#[pyfunction]
fn commit_transaction(py: Python, tx_id: u64) -> PyResult<&PyAny> { ... }

#[pyfunction]
fn rollback_transaction(py: Python, tx_id: u64) -> PyResult<&PyAny> { ... }
```

## Data Flow

### Query Execution

```
Python                          Rust
──────                          ────
User.objects.filter(age=18)
        │
        ▼
Query._build_filter_tree()
        │
        ▼
query.to_ir() → dict
        │
        ▼
msgpack.packb(ir) → bytes (~2KB)
        │
        ├────────────────────────▶ oxyde-codec: deserialize
        │                                  │
        │                                  ▼
        │                          oxyde-query: build_sql()
        │                                  │
        │                                  ▼
        │                          oxyde-driver: execute()
        │                                  │
        │                                  ▼
        │                          sqlx → Database
        │                                  │
        │                                  ▼
        │                          oxyde-driver: serialize results
        │                                  │
        ◀────────────────────────────────┘
        │
        ▼
msgpack.unpackb(result_bytes)
        │
        ▼
Pydantic: Model.model_validate()
        │
        ▼
User instances
```

### IR Format Example

```python
# Python query
User.objects.filter(age__gte=18, status="active").order_by("-created_at").limit(10)

# Generated IR (Python dict → MessagePack)
{
    "type": "select",
    "table": "users",
    "model": "myapp.models.User",
    "columns": ["id", "name", "email", "age", "status", "created_at"],
    "filter_tree": {
        "type": "and",
        "children": [
            {"type": "condition", "field": "age", "op": "gte", "value": 18},
            {"type": "condition", "field": "status", "op": "eq", "value": "active"}
        ]
    },
    "order_by": [["created_at", "desc"]],
    "limit": 10
}
```

## GIL Release

Rust async operations release Python's GIL:

```rust
#[pyfunction]
fn execute(py: Python, pool_name: String, ir_bytes: &[u8]) -> PyResult<&PyAny> {
    // Deserialize outside async (holds GIL)
    let ir: QueryIR = rmp_serde::from_slice(ir_bytes)?;

    // Release GIL for async I/O
    pyo3_asyncio::tokio::future_into_py(py, async move {
        // This runs without GIL
        let result = execute_query(&ir).await?;
        Ok(result)
    })
}
```

## Connection Registry

Global registry for connection pools:

```rust
pub struct ConnectionRegistry {
    pools: RwLock<HashMap<String, PoolHandle>>,
}

impl ConnectionRegistry {
    pub async fn insert(&self, name: String, handle: PoolHandle) -> Result<()> {
        let mut guard = self.pools.write().await;
        if guard.contains_key(&name) {
            return Err(DriverError::PoolAlreadyExists(name));
        }
        guard.insert(name, handle);
        Ok(())
    }

    pub async fn get(&self, name: &str) -> Result<PoolHandle> {
        let guard = self.pools.read().await;
        guard.get(name)
            .cloned()
            .ok_or_else(|| DriverError::PoolNotFound(name.to_string()))
    }
}
```

## Transaction Management

Transactions are stored in a separate registry:

```rust
pub struct TransactionRegistry {
    transactions: RwLock<HashMap<u64, TransactionInner>>,
    next_id: AtomicU64,
}

impl TransactionRegistry {
    pub async fn begin(&self, pool_name: &str) -> Result<u64> {
        let tx_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let pool = POOL_REGISTRY.get()?.get(pool_name).await?;
        let conn = begin_on_pool(&pool.pool, pool.backend).await?;

        let inner = TransactionInner {
            conn: Some(conn),
            state: TransactionState::Active,
            created_at: Instant::now(),
        };

        self.transactions.write().await.insert(tx_id, inner);
        Ok(tx_id)
    }
}
```

## Building from Source

```bash
# Build all crates
cargo build --release

# Build Python extension
cd crates/oxyde-core-py
maturin develop --release

# Run Rust tests
cargo test --workspace

# Run with logging
RUST_LOG=debug cargo test
```

## Debugging

### Enable Rust Logging

```bash
export RUST_LOG=info  # or debug, trace
python your_script.py
```

### Inspect IR

```python
query = User.objects.filter(age__gte=18)
ir = query.to_ir()
import json
print(json.dumps(ir, indent=2))
```

### Check SQL

```python
sql, params = query.sql(dialect="postgres")
print(sql)
print(params)
```

## Performance Considerations

### MessagePack Overhead

- Typical IR size: 1-3KB
- Serialization: ~50μs
- Deserialization: ~30μs
- Negligible compared to network I/O

### SQL Generation

- sea-query is highly optimized
- SQL building: ~10-50μs per query
- Cached prepared statements in sqlx

### Connection Pool

- Pool acquisition: ~1μs (cached)
- New connection: ~1-10ms (database dependent)
- Keep `min_connections > 0` for production

## Contributing

### Adding a New Operation

1. Define IR in `oxyde-codec`:
   ```rust
   pub struct NewOperationIR { ... }
   ```

2. Add SQL generation in `oxyde-query`:
   ```rust
   fn build_new_operation(ir: &NewOperationIR, dialect: Dialect) -> Result<...>
   ```

3. Expose in `oxyde-core-py`:
   ```rust
   #[pyfunction]
   fn new_operation(...) -> PyResult<...>
   ```

4. Add Python wrapper in `python/oxyde/queries/`

5. Rebuild:
   ```bash
   cd crates/oxyde-core-py && maturin develop --release
   ```

## Next Steps

- [Performance](performance.md) — Optimization techniques
- [Raw Queries](raw-queries.md) — Direct SQL execution
- [Connections](../guide/connections.md) — Connection configuration

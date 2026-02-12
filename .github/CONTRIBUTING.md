# Contributing to Oxyde

Thank you for your interest in contributing to Oxyde! This guide covers development setup, building from source, and running tests.

## Architecture Overview

Oxyde is a hybrid Python/Rust project:

```
┌──────────────────────────────────────────────────────────────┐
│  Python Layer (python/oxyde/)                                │
│  • Pydantic v2 models with database metadata                 │
│  • Django-like QuerySet API                                  │
│  • IR Builder (query → MessagePack)                          │
│  • Data validation (Pydantic validates, Rust executes)       │
└────────────────────────┬─────────────────────────────────────┘
                         │ MessagePack IR
                         ▼
┌──────────────────────────────────────────────────────────────┐
│  Rust Core (crates/)                                         │
│  • oxyde-codec: IR protocol & validation                     │
│  • oxyde-query: SQL generation via sea_query                 │
│  • oxyde-driver: Connection pools & execution via sqlx       │
│  • oxyde-migrate: Schema diff & migration generation         │
│  • oxyde-core-py: PyO3 bindings (Python ↔ Rust bridge)       │
└────────────────────────┬─────────────────────────────────────┘
                         │ SQL + parameters
                         ▼
┌──────────────────────────────────────────────────────────────┐
│  Database (PostgreSQL / SQLite / MySQL)                      │
└──────────────────────────────────────────────────────────────┘
```

### Rust Crates

| Crate | Purpose |
|-------|---------|
| `oxyde-codec` | MessagePack IR structures (QueryIR, FilterNode, Operation) |
| `oxyde-query` | Converts IR to database-specific SQL using sea_query |
| `oxyde-driver` | Connection pool management (sqlx), query execution |
| `oxyde-migrate` | Schema snapshots, diff computation, migration SQL |
| `oxyde-core-py` | PyO3 async bindings exposing Rust functions to Python |

### Python Package Structure

```
python/oxyde/
├── models/
│   ├── base.py         # Model, ModelMeta
│   ├── field.py        # Field(), OxydeFieldInfo
│   ├── decorators.py   # Index, Check
│   ├── metadata.py     # ColumnMeta, ForeignKeyInfo
│   ├── lookups.py      # Field lookups (__gte, __contains, etc.)
│   └── registry.py     # Model registry
├── queries/
│   ├── manager.py      # QueryManager (Model.objects interface)
│   ├── select.py       # Query (SelectQuery)
│   ├── insert.py       # InsertQuery
│   ├── q.py            # Q expressions (AND/OR/NOT)
│   ├── expressions.py  # F expressions (arithmetic)
│   ├── aggregates.py   # Count, Sum, Avg, Max, Min
│   └── mixins/         # Query method mixins
├── db/
│   ├── pool.py         # AsyncDatabase, PoolSettings
│   ├── transaction.py  # atomic() context manager
│   └── registry.py     # Connection registry
└── migrations/
    ├── cli.py          # makemigrations, migrate commands
    └── ...
```

## Prerequisites

- **Rust** 1.75+ ([rustup.rs](https://rustup.rs))
- **Python** 3.10+
- **maturin** (`pip install maturin`)
- **Database** (PostgreSQL/MySQL/SQLite for testing)

## Development Setup

### 1. Clone and Create Virtual Environment

```bash
git clone https://github.com/mr-fatalyst/oxyde.git
cd oxyde

python -m venv .venv
source .venv/bin/activate  # Linux/macOS
# or: .venv\Scripts\activate  # Windows
```

### 2. Build Rust Workspace

```bash
cargo build --release
```

### 3. Install Rust Python Extension

```bash
cd crates/oxyde-core-py
maturin develop --release
cd ../..
```

### 4. Install Python Package

```bash
cd python
pip install -e .[dev]
cd ..
```

## Development Workflow

### When Modifying Rust Code

After changing any Rust code, rebuild the Python extension:

```bash
# Quick rebuild (debug mode)
cd crates/oxyde-core-py
maturin develop

# Or with optimizations (release mode)
maturin develop --release
```

### When Modifying Python Code

No rebuild needed — changes are immediately available in editable install.

## Running Tests

### Rust Tests

```bash
# All crates
cargo test --workspace

# Specific crate
cargo test -p oxyde-query

# With output
cargo test --workspace -- --nocapture
```

### Python Tests

```bash
cd python

# All tests
pytest

# Specific file
pytest oxyde/tests/test_query.py

# Verbose
pytest -v

# With coverage
pytest --cov=oxyde
```

## Code Style

### Rust

```bash
# Format
cargo fmt

# Lint
cargo clippy --workspace
```

### Python

```bash
cd python

# Format and lint (using ruff)
ruff check --fix .
ruff format .

# Or run pre-commit
pre-commit run --all-files
```

## Debugging

### Enable Rust Logging

```bash
RUST_LOG=info python your_script.py
RUST_LOG=debug python your_script.py
RUST_LOG=oxyde_driver=debug python your_script.py
```

### Inspect MessagePack Payload

```python
import msgpack

query = User.objects.filter(age__gte=18)
ir_dict = query._build_ir().to_dict()
ir_bytes = msgpack.packb(ir_dict)
print(f"IR size: {len(ir_bytes)} bytes")
print(f"IR structure: {ir_dict}")
```

### Get Generated SQL

```python
query = User.objects.filter(age__gte=18).limit(10)
sql, params = query.sql()
print(f"SQL: {sql}")
print(f"Params: {params}")
```

## Adding New Features

### Adding a New Query Operation

1. **Define IR in Rust** (`crates/oxyde-codec/src/lib.rs`):
   ```rust
   pub struct NewOperationIR {
       pub field: String,
       pub value: Value,
   }
   ```

2. **Add SQL generation** (`crates/oxyde-query/src/lib.rs`):
   ```rust
   fn build_new_operation(ir: &NewOperationIR) -> Result<...> {
       // Use sea_query to build SQL
   }
   ```

3. **Expose to Python** (`crates/oxyde-core-py/src/lib.rs`):
   ```rust
   #[pyfunction]
   fn execute_new_operation(py: Python<'_>, ir_bytes: &[u8]) -> PyResult<...> {
       // Deserialize IR, call driver
   }
   ```

4. **Add Python API** (`python/oxyde/queries/`):
   ```python
   def new_operation(self, **kwargs):
       # Build IR, call Rust
   ```

5. **Rebuild extension**:
   ```bash
   cd crates/oxyde-core-py && maturin develop --release
   ```

6. **Add tests** for both Rust and Python.

## Common Pitfalls

### Forgetting to Rebuild After Rust Changes

Python won't see Rust changes until `maturin develop` is run. Symptom: old behavior persists.

### GIL-Related Performance Issues

Rust async operations release the GIL via `pyo3_asyncio::tokio::future_into_py`. Don't add unnecessary Python callbacks in hot paths.

### MessagePack Size Limits

IR payload should stay under 10KB for best performance. Large bulk operations may need batching.

### SQLite Connection Limits

SQLite doesn't benefit from large connection pools. Use `max_connections=1` or rely on WAL mode with limited concurrency.

## Pull Request Guidelines

1. **Create a branch** from `main`
2. **Write tests** for new functionality
3. **Run all tests** before submitting
4. **Format code** with `cargo fmt` and `ruff`
5. **Update documentation** if needed
6. **Keep commits atomic** — one logical change per commit

## Questions?

- Open an issue on GitHub
- Check existing issues for similar questions

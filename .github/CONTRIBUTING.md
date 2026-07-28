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
│  • oxyde-sql: SQL generation via sea_query                 │
│  • oxyde-driver: Connection pools & execution via sqlx       │
│  • oxyde-migrate: Schema diff computation                    │
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
| `oxyde-codec` | Serialized contracts: QueryIR, ColumnTypeSpec, migration ops/snapshots |
| `oxyde-sql` | All SQL generation via sea_query: DML from IR + migration DDL |
| `oxyde-driver` | Connection pool management (sqlx), query execution |
| `oxyde-migrate` | Schema diff computation |
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
├── core/
│   ├── wrapper.py      # Bridge to the compiled _oxyde_core module
│   ├── ir.py           # IR builders (query state → dict for msgpack)
│   ├── column_types.py # THE single Python → ColumnTypeSpec mapping point
│   └── types.py        # Value serialization for msgpack
├── db/
│   ├── pool.py         # AsyncDatabase, PoolSettings
│   ├── transaction.py  # atomic() context manager
│   └── registry.py     # Connection registry
├── migrations/         # Detection, generation, replay, squash, execution
└── cli/
    └── migrations.py   # makemigrations / migrate / sqlmigrate / squash
```

## Prerequisites

- **Rust** via [rustup.rs](https://rustup.rs) — the exact toolchain is pinned in
  `rust-toolchain.toml` and rustup installs it automatically on first `cargo` use.
  CI uses the same pinned version, so a floating local stable may show clippy
  warnings CI does not (or vice versa) — always build through rustup.
- **Python** 3.10+
- **maturin** (`pip install maturin`)
- **Docker** — integration tests spin up PostgreSQL/MySQL via testcontainers;
  without Docker they are skipped and only unit/smoke tests run.

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

Day-to-day commands are `make` targets (see the `Makefile` at the repo root):

| Command | What it does |
|---|---|
| `make build-core` | Rebuild the Rust extension (`maturin develop --release`) |
| `make test-rust` | All Rust tests (`cargo test --workspace`) |
| `make test` | All Python tests (integration auto-skips without Docker) |
| `make test-unit` / `test-smoke` / `test-integration` | One Python suite |
| `make coverage` | Python tests with coverage |
| `make lint` | Everything CI checks: ruff, mypy, cargo fmt/check/clippy |
| `make format` | Format both languages (ruff format + cargo fmt) |

### When Modifying Rust Code

After changing any Rust code, rebuild the Python extension:

```bash
make build-core
```

### When Modifying Python Code

No rebuild needed — changes are immediately available in editable install.

## Running Tests

### Rust Tests

```bash
make test-rust

# Narrower runs go through cargo directly:
cargo test -p oxyde-sql
cargo test --workspace -- --nocapture
```

#### Golden DDL snapshots

`crates/oxyde-sql/tests/golden_ddl.rs` freezes the generated DDL byte-exact
(via [insta](https://insta.rs)). If your change alters generated SQL, the test
fails with a diff — review it and accept **consciously**:

```bash
cargo test -p oxyde-sql --test golden_ddl   # shows the diff
cargo insta review                          # accept/reject interactively
```

An accepted snapshot diff must be part of the same PR and called out in its
description: changed DDL bytes are a feature decision, never a side effect.

### Python Tests

Tests are split into suites under `python/oxyde/tests/`:

- `unit/` — no database needed
- `smoke/` — quick end-to-end sanity on SQLite
- `integration/` — real PostgreSQL/MySQL via testcontainers (needs Docker)
- `typecheck/` — mypy against generated stubs

```bash
make test          # all (integration is skipped automatically without Docker)
make test-unit     # or test-smoke / test-integration
make coverage

# A single file goes through pytest directly:
cd python && pytest oxyde/tests/unit/test_query.py
```

## Code Style

```bash
make lint
```

This runs every check CI runs (ruff, mypy, cargo fmt/check/clippy). Run it
before pushing — CI enforces `cargo clippy -- -D warnings` with the `pedantic`
lint group enabled, so a clean local `make lint` is the only reliable way to
avoid a red pipeline. `make format` applies formatting to both languages.

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
ir_dict = query.to_ir()
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

2. **Add SQL generation** (`crates/oxyde-sql/src/lib.rs`):
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
   make build-core
   ```

6. **Add tests** for both Rust and Python.

### Adding a New Column Type

Column type semantics live in exactly two places — keep it that way:

1. **Add a variant to `ColumnTypeSpec`** (`crates/oxyde-codec/src/lib.rs`) —
   the tagged enum that travels from Python to Rust.
2. **Map it in Rust** (`crates/oxyde-sql/`):
   - `src/spec_sql.rs` — DDL type string per dialect (`resolve_spec_type`);
   - `src/utils/bind.rs` — value binding + typed NULL for the new kind;
   - `crates/oxyde-driver/src/convert/{postgres,mysql,sqlite}.rs` — row decoding.
3. **Map it in Python** (`python/oxyde/core/column_types.py`) — the single
   Python-side classification point (`compute_column_type`). Nothing else in
   the Python layer may classify types.
4. **Freeze the DDL**: extend `crates/oxyde-sql/tests/golden_ddl.rs` and record
   the snapshots (see the golden workflow above), plus binding cases in
   `bind.rs` tests and `python/oxyde/tests/unit/test_db_types.py`.

## Common Pitfalls

### Forgetting to Rebuild After Rust Changes

Python won't see Rust changes until `make build-core` is run. Symptom: old behavior persists.

### GIL-Related Performance Issues

Rust async operations release the GIL via `pyo3_asyncio::tokio::future_into_py`. Don't add unnecessary Python callbacks in hot paths.

### MessagePack Size Limits

IR payload should stay under 10KB for best performance. Large bulk operations may need batching.

### SQLite Connection Limits

SQLite doesn't benefit from large connection pools. Use `max_connections=1` or rely on WAL mode with limited concurrency.

## Pull Request Guidelines

1. **Build the Rust core from source** before submitting a fix. The `oxyde-core` package on PyPI may be behind `main` — your issue might already be fixed. Always run `make build-core` first.
2. **Create a branch** from `main`
3. **Write tests** for new functionality
4. **Run `make test-rust`, `make test` and `make lint`** before submitting — CI runs exactly these
5. **Format code** with `make format`
6. **Update documentation** if needed
7. **Keep commits atomic** — one logical change per commit

## Questions?

- Open an issue on GitHub
- Check existing issues for similar questions

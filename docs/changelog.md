# Changelog

All notable changes to Oxyde are documented here.

---

## 0.5.1 - 2026-03-08

### Bug Fixes

- **`Optional["Model"]` not detected as FK** — `Optional["Author"]` (with a string forward reference) was not recognized as a foreign key because `ForwardRef` was not handled in the metaclass FK detection. Now works correctly alongside `Author | None` and `from __future__ import annotations`.
- **Python 3.14 compatibility** — FK detection relied on reading `namespace["__annotations__"]` in the metaclass `__new__`, which is empty on Python 3.14 (PEP 749: lazy annotations). Moved FK detection after class creation, using Pydantic's `model_fields` which works on all Python versions.

### Improvements

- **Metaclass cleanup** — extracted `_build_globalns()` and `_get_db_attr()` helpers, reducing duplication in `_resolve_fk_fields()` and `_parse_field_tags()`. Relation field detection uses `OxydeFieldInfo.is_virtual` property.

### Documentation

- **Fixed FK annotation examples** — replaced `"Author" | None` (TypeError on Python 3.10-3.13) with `Author | None` across all docs. Added a detailed note on type annotations, forward references, and `from __future__ import annotations`.

---

## 0.5.0 - 2026-03-07

### Breaking Changes

#### `update()` returns row count by default

`update()` now returns `int` (number of affected rows) instead of `list[dict]`. This aligns with Django's `update()` behavior. Pass `returning=True` to get the previous behavior.

```python
# Before (0.4.x)
rows = await Post.objects.filter(id=42).update(status="published")
# rows was list[dict]

# After (0.5.0)
count = await Post.objects.filter(id=42).update(status="published")
# count is int

rows = await Post.objects.filter(id=42).update(status="published", returning=True)
# rows is list[dict] (explicit opt-in)
```

#### `execute_to_pylist()`, `execute_batched_dedup()` removed from `AsyncDatabase`

The Python-side batched/dedup execution methods have been removed. JOIN deduplication is now handled entirely in the Rust core encoder. Use `execute()` for all queries.

#### `batch_size` removed from `PoolSettings`

The `batch_size` parameter is no longer needed since batching is handled by the Rust core.

---

### Bug Fixes

- **`distinct` ignored in aggregates** — `Count("field", distinct=True)` (and `Sum`, `Avg`) now correctly generates `COUNT(DISTINCT field)`. The `distinct` flag was not propagated through the IR.
- **`refresh()` overwrote virtual fields** — calling `refresh()` on a model instance with `reverse_fk` or `m2m` relations would overwrite them with raw data from the database. Virtual relation fields are now skipped.
- **`timedelta` not deserialized** — `timedelta` columns came back as raw integers (microseconds). Now correctly converted back to `datetime.timedelta` objects on the Python side.
- **`group_by()` produced incorrect SQL** — custom group-by implementation replaced with native sea-query group-by, fixing edge cases with table qualification and JOIN queries.
- **`union()` / `union_all()` rewritten** — custom union SQL generation replaced with native sea-query `UNION` support. Fixes ordering and parenthesization issues. Union sub-queries are now built recursively.
- **`datetime` cast incorrect** — fixed datetime value conversion in the Rust driver layer.
- **Binary data corrupted through serde** — binary fields (`bytes`) were mangled by the `serde_json::Value` intermediate layer. Replaced by `rmpv::Value` for lossless binary round-trip.

---

### Improvements

- **serde_json eliminated from data path** — the entire row encoding pipeline now goes directly from sqlx rows to MessagePack via `rmpv`, removing the `serde_json::Value` intermediate representation. This fixes binary/timedelta data corruption and reduces allocations.
- **`CellEncoder` trait** — new unified trait in `oxyde-driver` for columnar row encoding. Each backend (Postgres, SQLite, MySQL) implements `CellEncoder` with type-specific decoding; generic functions handle the columnar structure.
- **JOIN dedup moved to Rust** — relation deduplication for JOIN queries is now performed in the Rust encoder (`encoder.rs`), replacing the Python-side `execute_batched_dedup` path. Results use a compact 3-element msgpack format: `[main_columns, main_rows, relations_map]`.
- **`having()` supports annotation aliases** — `having(total__gt=100)` now correctly resolves `total` from `annotate(total=Sum(...))` instead of treating it as a model field. Supported lookups: `exact`, `gt`, `gte`, `lt`, `lte`.
- **`group_by()` guards model hydration** — calling `.all()` on a `group_by()` query now raises `TypeError` with a clear message suggesting `.values()` or `.fetch_all()` instead.
- **Aggregate `DISTINCT` support** — `Count`, `Sum`, and `Avg` now accept `distinct=True` at both the IR and SQL generation levels. `Max`/`Min` ignore it (as `DISTINCT` is meaningless for those).
- **Rust crate modularization** — `oxyde-core-py`, `oxyde-driver`, and `oxyde-migrate` monolithic `lib.rs` files split into focused modules (`convert.rs`, `execute.rs`, `pool.rs`, `migration.rs`, `diff.rs`, `op.rs`, `sql.rs`, etc.).
- **Migrations use native sea-query** — migration SQL generation now uses sea-query builders instead of hand-crafted SQL strings, improving dialect compatibility.
- **Migration code deduplicated** — shared utilities (`detect_dialect`, `load_migration_module`, `parse_query_result`) extracted to `oxyde.migrations.utils`.
- **Test suite restructured** — tests organized into `unit/`, `smoke/`, and `integration/` directories with shared helpers (`StubExecuteClient`). New integration tests cover CRUD, aggregation, filtering, pagination, relations, transactions, field types, and edge cases against real SQLite.
- **Free-threaded Python support** — CI now builds wheels for Python 3.13t and 3.14t (free-threaded / no-GIL builds).
- **`typer` bumped to >= 0.24** — resolves deprecation warnings.

---

## 0.4.0 - 2026-02-23

### Breaking Changes

#### `OxydeModel` renamed to `Model`

The base class has been renamed from `OxydeModel` to `Model`:

```python
# Before (0.3.x)
from oxyde import OxydeModel

class User(OxydeModel):
    ...

# After (0.4.0+)
from oxyde import Model

class User(Model):
    ...
```

!!! warning "Deprecation notice"
    `OxydeModel` still works in 0.4.x and emits a `DeprecationWarning`. It will be **removed** in a future release. Update your imports now:

    ```
    # Find and replace across your project:
    OxydeModel → Model
    ```

    Direct import `from oxyde.models.base import OxydeModel` is **not supported** and will raise `ImportError`.

#### `limit()` and `offset()` reject negative values

Both methods now raise `ValueError` on negative input. Previously negative values were silently accepted and produced invalid SQL.

```python
# 0.3.x: silently generated broken SQL
qs.limit(-1)

# 0.4.0: raises ValueError
qs.limit(-1)  # ValueError: limit() requires a non-negative value, got -1
```

#### `ensure_field_metadata()` removed

The classmethod `Model.ensure_field_metadata()` has been removed. Model metadata is now finalized eagerly at class definition time. If you were calling this method manually, simply remove the call — it is no longer needed.

#### `resolve_pending_fk()` replaced

`resolve_pending_fk()` in `oxyde.models.registry` has been replaced by:

- `finalize_pending()` — eagerly finalizes all pending models
- `assert_no_pending_models()` — raises `RuntimeError` if any models are still pending

#### `migrations.types` module removed

The `oxyde.migrations.types` module (`validate_sql_type`, `normalize_sql_type`, `translate_db_specific_type`) has been removed. Type handling is now internal to the Rust core.

---

### Bug Fixes

- **`union()` / `union_all()` were silently ignored** — union queries now correctly generate SQL and execute as expected.
- **FK relation fields included in INSERT/UPDATE** — fields like `author: Author` were incorrectly serialized into INSERT/UPDATE statements alongside the actual `author_id` column. Now properly excluded.
- **Nested JOIN hydration failed** — deeply nested joins (e.g. `post__author__profile`) could produce incorrect or missing data. Hydration logic rewritten to correctly resolve nested FK references.
- **`get_or_create()` race condition** — concurrent calls creating the same record could both fail with `IntegrityError`. Now retries `get()` on conflict.
- **Migration advisory lock on wrong connection** — `pg_try_advisory_lock` and `pg_advisory_unlock` could execute on different pool connections. Lock now pins a connection via `begin_transaction()`.
- **Transaction leak on savepoint failure** — if savepoint creation failed, transaction depth was already incremented, corrupting state. Depth is now incremented only after successful savepoint creation.
- **`bulk_create([])` did not raise** — empty list was silently accepted. Now correctly raises `ValueError`.
- **Negative pool duration accepted** — `PoolSettings` durations now raise `ValueError` for negative values.
- **`makemigrations` silently continued on broken migrations** — if replaying existing migrations failed, the CLI used an empty schema as baseline, potentially generating destructive migrations. Now fails with exit code 1.
- **`migrate` wrong exit code** — `migrate` could exit with code 0 when migration was not found.

---

### Improvements

- **Eager model finalization** — model metadata (field metadata, column types, PK info) is now computed at class definition time instead of lazily on first query. Eliminates an entire class of "metadata not ready" bugs.
- **Unified type registry** — `TYPE_REGISTRY` in `oxyde.core.types` consolidates all Python-to-IR type mappings. Fixes `bool` being misclassified as `int` in lookups.
- **Local imports removed** — circular dependency issues resolved; all imports are now at module level for better readability and faster import time.

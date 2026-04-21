# Changelog

All notable changes to Oxyde are documented here.

---

## 0.7.0 - Unreleased

**Rust core: 0.6.1** (`core-v0.6.1`)

### Breaking Changes

#### `update(returning=True)` returns model instances

`update(returning=True)` now returns `list[Model]` instead of `list[dict]`. Returned rows are validated through Pydantic and hydrated as full model instances.

```python
# Before (0.6.x)
rows = await Post.objects.filter(id=42).update(status="published", returning=True)
# rows was list[dict]

# After (0.7.0)
posts = await Post.objects.filter(id=42).update(status="published", returning=True)
# posts is list[Model]
```

#### Default type mapping changes

Several default type mappings have changed:

| Python Type | Dialect | Before | After |
|-------------|---------|--------|-------|
| `int` (non-PK) | PostgreSQL | `BIGINT` | `INTEGER` |
| `str` | All | `TEXT` | `VARCHAR(255)` |
| `Decimal` | SQLite | `NUMERIC` | `TEXT` |
| `datetime` | MySQL | `DATETIME` | `DATETIME(6)` |
| `time` | MySQL | `TIME` | `TIME(6)` |
| `bytes` | MySQL | `BLOB` | `LONGBLOB` |

!!! note "Existing databases"
    These changes affect **new migrations only**. Existing columns are not altered automatically. If you need the old types, use `db_type` override: `Field(db_type="BIGINT")`.

### New Features

- **`create_tables()` / `drop_tables()`** — programmatic schema management without migration files. Generates dialect-aware DDL from registered models and executes it directly. Useful for tests and scripts.

    ```python
    from oxyde import create_tables, drop_tables

    await create_tables(database)   # CREATE all registered model tables
    await drop_tables(database)     # DROP all registered model tables
    ```

- **`update_or_create()`** — get an existing record by filters and update it with `defaults`, or create a new one if not found. Race-condition safe (catches `IntegrityError`, retries `get`).

    ```python
    user, created = await User.objects.update_or_create(
        email="alice@example.com",
        defaults={"name": "Alice", "age": 31},
    )
    ```

- **Random ordering** — `order_by("?")` generates `RANDOM()` (PostgreSQL/SQLite) or `RAND()` (MySQL).

    ```python
    random_users = await User.objects.order_by("?").limit(5).all()
    ```

- **SQL wildcard escaping in LIKE lookups** — `contains`, `startswith`, `endswith` and their case-insensitive variants now escape literal `%`, `_` and `\` characters with an `ESCAPE` clause. Previously `filter(name__contains="50%")` would treat `%` as a wildcard.

- **TLS/SSL for connections** — new `PoolSettings` fields for encrypted connections:

    ```python
    PoolSettings(
        ssl_mode="require",                       # PG / MySQL mode
        ssl_root_cert="/path/to/ca.pem",          # CA certificate
        ssl_client_cert="/path/to/client.crt",    # mTLS client cert
        ssl_client_key="/path/to/client.key",     # mTLS client key
    )
    ```

- **PostgreSQL pool settings** — `pg_application_name` (visible in `pg_stat_activity`) and `pg_statement_cache_capacity` (prepared statement cache size).

- **MySQL pool settings** — `mysql_charset` and `mysql_collation`.

- **Decimal field constraints** — `max_digits` and `decimal_places` in `Field()` now generate `DECIMAL(M,D)` / `NUMERIC(M,D)` in migrations. Changes to these constraints are detected and produce `ALTER COLUMN`.

    ```python
    price: Decimal = Field(max_digits=10, decimal_places=2)
    # PostgreSQL: NUMERIC(10,2)  MySQL: DECIMAL(10,2)
    ```

- **Array inner type constraints** — array fields preserve inner type constraints in DDL: `list[Annotated[str, Field(max_length=50)]]` generates `VARCHAR(50)[]` on PostgreSQL.

### Bug Fixes

- **`update(returning=True)` on MySQL** — MySQL doesn't support `RETURNING`. Now uses an atomic `SELECT ... FOR UPDATE` → `UPDATE` → re-fetch pattern within a transaction, transparent to the caller.
- **`iexact` lookup didn't escape wildcards** — `_` and `%` in `iexact` values were treated as SQL wildcards. Now escaped with `ESCAPE` clause, matching `contains`/`startswith`/`endswith` behavior.
- **`@computed_field` included in INSERT/UPDATE** — Pydantic computed fields were serialized into mutation queries, causing validation errors. Now excluded.
- **Aggregate return types** — PostgreSQL returns `NUMERIC` as string via msgpack. Aggregates now coerce results to the field's Python type (`int`, `float`, `Decimal`).
- **Insert guard ignored RETURNING in transactions** — the code checked `ir.returning` flag but not whether the generated SQL actually contained `RETURNING`, causing incorrect execution path on MySQL within transactions.
- **MySQL transactions** — transaction SQL (`COMMIT`, `ROLLBACK`, `SAVEPOINT`) refactored from `sqlx::query().execute()` to `conn.execute()` for MySQL compatibility.
- **MySQL decimal** — decimal values were extracted as `String` instead of `rust_decimal::Decimal`, causing type mismatches.
- **MySQL DATETIME/TIME precision** — now generates `DATETIME(6)` and `TIME(6)` for microsecond precision.
- **`db_type` array handling** — array type constraints (e.g. `varchar(100)`) were not stripped before type lookup, causing encoding failures.
- **INSERT RETURNING on MySQL** — targeted Rust driver fix so `create()` / `bulk_create(..., returning=True)` take the correct execution path on MySQL (which has no native `RETURNING`).
- **Stub generator emitted invalid syntax for bare types** — unparameterized `dict`, `list`, `tuple`, `set` and similar produced stubs that failed to parse. Now emitted correctly.
- **Stub generator crashed on parameterized generics on older Python** — `isinstance()` against a subscripted generic raises `TypeError` on Python 3.10/3.11. The generator now checks `__origin__` first.

### Migrations

- **Tables are topologically sorted by FK dependencies** — `compute_diff` now orders `CREATE TABLE` statements so referenced tables are created before referencing ones; cyclic FK graphs are rejected with a clear error.
- **Minimal payload for drop operations** — `drop_foreign_key`, `drop_index` and `drop_check` ops no longer require the full column/constraint definition, only the name. (#24)
- **`alter_column` replay preserves constraints** — `max_length`, `max_digits` and `decimal_places` survive migration replay; previously they were dropped when a column was rewritten by replay. (#25)

### Internal

- **mypy compliance** — fixed mypy errors across the codebase; mypy added to pre-commit hooks.
- **Type overloads** for `update()`: `returning: Literal[True] → list[Model]`, `returning: Literal[False] → int`.
- **Stub generator** updated with `update_or_create()` signature.
- **Migration operation ordering** — `CREATE`/`DROP`/`INDEX` statements execute before `ALTER TABLE` (fixes FK constraints on PostgreSQL/MySQL).
- **Constraint change detection** — migration diff detects changes in `max_length`, `max_digits`, `decimal_places`.
- **Pool backend detection** — new Rust function `pool_backend()` exposed to Python for runtime database type detection.
- **Stub generator rewritten on top of `ast`** — the generator previously relied on regex-based parsing; it now walks the AST, producing more reliable output and eliminating several classes of malformed stubs.
- **Rust core bumped to 0.6.1** (`core-v0.6.1`).

---

## 0.6.1 - 2026-03-24

### Bug Fixes

- **`count()` and `exists()` bypassed `to_ir()`** — both methods built their IR directly via `ir.build_select_ir()`, skipping the full `to_ir()` pipeline. This meant `col_types` and other query state were not included, which could produce incorrect SQL when type-sensitive filters were involved. Both now go through `to_ir()` like all other query methods.
- **`save(update_fields)` silently ignored FK field names** — passing a virtual FK name like `update_fields=["author"]` was silently accepted but the field was not included in the UPDATE because it didn't match any database column. Now correctly resolves FK names to their synthetic columns (e.g. `"author"` → `"author_id"`). Virtual relation fields (`reverse_fk`, `m2m`) now raise `FieldError`.

### Internal

- **`TQuery` TypeVar replaced with `Self`** — all query mixins (`FilteringMixin`, `PaginationMixin`, `AggregationMixin`, `ExecutionMixin`, `JoiningMixin`, `DebugMixin`) and `Query` now use `typing_extensions.Self` for return-type annotations instead of the custom `TQuery` TypeVar. Fixes mypy errors with the previous approach.

---

## 0.6.0 - 2026-03-20

### Bug Fixes

- **`bulk_update()` used field names instead of `db_column` as filter keys** — when a model had a custom `db_column`, the generated SQL referenced the Python field name, which doesn't exist in the database. Now correctly maps field names to `db_column` via model metadata.
- **Result columns not remapped to field names** — Rust returns `db_column` names (e.g. `author_id`), but Python expects field names (`author`). Added `reverse_column_map` to `ModelMeta` and `_remap_columns()` in the execution layer. Affects all result formats: dedup, columnar, and row dicts.
- **`Decimal` bound as string on PostgreSQL** — `Decimal` values were serialized as strings, causing type mismatches in comparisons. Now bound natively via `rust_decimal::Decimal`.
- **`TIMESTAMPTZ` not decoded correctly** — timezone-aware datetime columns lost timezone info during encoding. Now preserves timezone in RFC3339 format.
- **`datetime` strings falsely parsed as datetime** — ISO 8601 strings stored in `VARCHAR`/`TEXT` columns (e.g. `"2024-01-15T12:30:00Z"`) were auto-parsed into datetime values. Type hints now prevent this: if `col_type` is `"str"` or `"TEXT"`, the value stays as a string.
- **`timedelta` encoded as integer microseconds** — timedelta values were exposed as raw `i64` microseconds. Now correctly converted to `f64` seconds at the driver layer, matching Python's `timedelta` representation.
- **Type-unsafe filter binding** — filter values were converted without consulting column type hints, leading to implicit string coercion (e.g. `age = '18'` instead of `age = 18`). Filter builder now receives `col_types` and binds values with correct SQL types.
- **`DELETE` path ignored type hints** — value conversion for DELETE queries did not pass `col_types`, causing the same coercion issues as filters. Fixed.

### New Features

- **PostgreSQL array support** — native support for `int[]`, `str[]`, `uuid[]`, `bool[]`, `float[]`, `decimal[]`, `datetime[]` columns. Arrays are correctly bound as parameters, decoded from results (including NULL elements), and mapped in migrations (`BIGINT[]` on Postgres, `JSON` on MySQL, `TEXT` on SQLite).
- **JSON column support** — `dict` fields are now mapped to `JSONB` (Postgres), `JSON` (MySQL), `TEXT` (SQLite) in migrations. Values are bound as `Value::Json` in queries.
- **`.sql(with_types=True)`** — new optional parameter shows exact SQL type tags for each bound parameter: `[("BigInt", 18), ("Uuid", "550e...")]`. Useful for debugging type binding issues.

### Improvements

- **Centralized type classification in Rust** — new `classify_type()` function as single source of truth for mapping IR type names and SQL type names to semantic categories. Replaces scattered match arms across the codebase.
- **Typed NULL values** — `NULL` parameters now carry their column type (e.g. `Value::Uuid(None)` instead of generic `Value::String(None)`), preventing type mismatch errors on strict databases.
- **`BIGSERIAL` for integer PKs on PostgreSQL** — integer primary keys now map to `BIGSERIAL` instead of `SERIAL`, supporting larger sequences out of the box.
- **`reverse_column_map` cached on `ModelMeta`** — `db_column → field_name` mapping is computed once at model finalization, not on every query.

### Internal

- **Rust core bumped to 0.5.0** (core-v0.5.0).
- Removed `_convert_timedelta_columns()` from Python execution layer (moved to Rust driver).
- Removed field_name aliasing from Rust SELECT; column remapping now happens in Python via `reverse_column_map`.
- Unified `_get_python_type_name` to delegate to `get_ir_type`. Added `json` and `T[]` patterns to Rust `python_type_to_sql`.
- Added `test_type_binding` test suite covering type classification, array handling, typed nulls, and edge cases.
- Added datetime filtering integration tests.

---

## 0.5.2 - 2026-03-13

### Bug Fixes

- **`create()` / `save()` now validates returned data via Pydantic** — `RETURNING *` results were applied via raw `setattr`, bypassing Pydantic validation and type coercion. Additionally, `save()` on new instances did not update the object with auto-generated fields (PK, defaults) after insertion. Both paths now use `model_validate()`.
- **`save()` on MySQL no longer raises `NotFoundError`** — `save()` for existing records always requested `RETURNING`, which MySQL doesn't support. Now detects the backend and falls back to affected row count for MySQL.
- **`bulk_create()` no longer loses fields across rows** — `_dump_insert_data()` used `exclude_none=True`, which couldn't distinguish "user didn't set the field" from "user explicitly set None". Changed to `exclude_unset=True` so explicitly passed `None` is preserved while unset fields are omitted (letting DB defaults work).
- **`bulk_update()` no longer corrupts `bytes` fields** — `model_dump(mode="json")` converted bytes to base64 strings, which Rust wrote as `VARCHAR` instead of binary. Now uses `mode="python"` with `_serialize_value_for_ir()`, matching the same serialization path as `update()`.
- **`annotate()` rejects invalid expressions** — passing an object without `to_ir()` (e.g. `annotate(x=object())`) was silently ignored. Now raises `TypeError`, matching Django's behavior.
- **`bulk_create(batch_size=0)` raises `ValueError`** — previously crashed with an unhelpful `range()` error. Negative values silently skipped the INSERT. Now validates `batch_size > 0`.
- **ContextVar transaction leak** — child async tasks inherited the parent's transaction scope via `ContextVar`. Added `_get_owned_entry()` ownership check using `asyncio.current_task()` so child tasks no longer see the parent's transaction.

### Improvements

- **Streaming query execution** — replaced `fetch_all()` with `fetch()` streams in the Rust driver. Rows are now encoded to MessagePack incrementally as they arrive from the database, reducing peak memory usage for large result sets. (core-v0.4.2)
- **Unified migration ordering** — `apply_migrations()`, `replay_migrations_up_to()`, and `get_pending_migrations()` now use topological sort via `depends_on`, matching the behavior already used in `replay_migrations()`. Previously these paths used lexicographic sorting only.

### Internal

- Removed dead `get_migration_order()` function from `replay.py`.
- Moved local imports to module level in migration integration tests.

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

# Changelog

All notable changes to Oxyde are documented here.

---

## 0.4.0

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

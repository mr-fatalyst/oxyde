"""Squash migration history into a single initial migration.

Replays all migration files in memory, computes the final schema, and
replaces the whole history with one ``0001_<name>.py`` written in the
current (ColumnTypeSpec) format. This is the conversion path the legacy
format FutureWarning points to; legacy-format support is removed in 1.0.

Manual operations (``ctx.execute()`` with raw SQL) are schema-neutral and
are NOT carried over — affected files are reported so the user can move
that logic manually.

Old files are deleted after the new file is generated successfully
(version control is the backup). The new file is rendered into a temporary
directory first, so a generation failure cannot lose existing history.

For already-deployed databases the new initial migration must be recorded
without executing: ``oxyde migrate --fake``. Orphaned tracker records of
the old migration names are harmless (pending = files − applied).
"""

from __future__ import annotations

import json
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

from oxyde.core import migration_compute_diff
from oxyde.migrations.context import MigrationContext
from oxyde.migrations.generator import generate_migration_file
from oxyde.migrations.replay import SchemaState, _topological_sort_migrations
from oxyde.migrations.utils import load_migration_module, op_uses_legacy_fields


@dataclass
class SquashResult:
    """Outcome of a squash run."""

    new_file: Path | None
    deleted_files: list[str] = field(default_factory=list)
    raw_sql_files: list[str] = field(default_factory=list)
    legacy_files: list[str] = field(default_factory=list)
    table_count: int = 0


def squash_migrations(
    migrations_dir: str | Path,
    name: str = "squashed",
) -> SquashResult:
    """Squash all migrations in ``migrations_dir`` into one initial file.

    Returns a SquashResult; ``new_file is None`` means there was nothing
    to squash (no migration files found).
    """
    migrations_path = Path(migrations_dir)
    files = sorted(migrations_path.glob("[0-9]*.py"))
    if not files:
        return SquashResult(new_file=None)

    # Replay history per file, tracking raw-SQL and legacy-format usage
    state = SchemaState()
    raw_sql_files: list[str] = []
    legacy_files: list[str] = []

    for file in _topological_sort_migrations(files):
        module = load_migration_module(file)
        if module is None or not hasattr(module, "upgrade"):
            continue
        ctx = MigrationContext(mode="collect")
        module.upgrade(ctx)
        if ctx.has_raw_sql:
            raw_sql_files.append(file.name)
        operations = ctx.get_collected_operations()
        if any(op_uses_legacy_fields(op) for op in operations):
            legacy_files.append(file.name)
        for op in operations:
            state.apply_operation(op)

    # Final schema → create ops via the Rust diff (empty → state):
    # CreateTable operations come out topologically sorted by FK deps.
    snapshot = state.to_snapshot()
    empty = {"version": 1, "tables": {}}
    ops_json = migration_compute_diff(json.dumps(empty), json.dumps(snapshot))
    ops = json.loads(ops_json)

    # Render into a temp dir first: numbering restarts at 0001 and a
    # generation failure cannot lose the existing history.
    with tempfile.TemporaryDirectory() as tmp:
        tmp_file = generate_migration_file(ops, migrations_dir=tmp, name=name)
        content = tmp_file.read_text()
        new_name = tmp_file.name

    deleted = [f.name for f in files]
    for f in files:
        f.unlink()

    new_file = migrations_path / new_name
    new_file.write_text(content)

    return SquashResult(
        new_file=new_file,
        deleted_files=deleted,
        raw_sql_files=raw_sql_files,
        legacy_files=legacy_files,
        table_count=len(snapshot["tables"]),
    )


__all__ = ["SquashResult", "squash_migrations"]

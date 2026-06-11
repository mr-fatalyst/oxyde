"""Shared utilities for the migration system."""

from __future__ import annotations

import importlib.util
import sys
from collections.abc import Iterator
from pathlib import Path
from typing import Any

from oxyde._msgpack import msgpack


def detect_dialect(url: str) -> str:
    """Detect database dialect from connection URL.

    Args:
        url: Database connection URL (e.g. "postgresql://...", "sqlite:///...")

    Returns:
        Dialect string: "sqlite", "postgres", or "mysql"
    """
    url_lower = url.lower()
    if url_lower.startswith("sqlite"):
        return "sqlite"
    if url_lower.startswith("postgres"):
        return "postgres"
    if url_lower.startswith("mysql") or url_lower.startswith("mariadb"):
        return "mysql"
    return "postgres"


def load_migration_module(filepath: Path) -> Any | None:
    """Load a migration module from file.

    Args:
        filepath: Path to migration .py file

    Returns:
        Loaded module, or None if loading failed
    """
    spec = importlib.util.spec_from_file_location(filepath.stem, filepath)
    if spec is None or spec.loader is None:
        return None

    module = importlib.util.module_from_spec(spec)
    sys.modules[filepath.stem] = module
    spec.loader.exec_module(module)
    return module


def parse_query_result(result_bytes: bytes) -> list[dict[str, Any]]:
    """Parse MessagePack query result into list of dicts.

    Args:
        result_bytes: Raw MessagePack bytes from query

    Returns:
        List of row dicts with column names as keys
    """
    if not result_bytes:
        return []

    result = msgpack.unpackb(result_bytes, raw=False)

    # Format: [columns, rows] where columns is list of names, rows is list of lists
    if isinstance(result, list) and len(result) == 2:
        columns, rows = result
        if isinstance(columns, list) and isinstance(rows, list):
            return [dict(zip(columns, row)) for row in rows]

    # Fallback: already list of dicts
    if isinstance(result, list) and all(isinstance(r, dict) for r in result):
        return result

    return []


# ── Legacy field-format normalization ────────────────────────────────────


def normalize_field_dict(field: dict) -> dict:
    """Ensure a FieldDef dict carries ``column_type`` (ColumnTypeSpec).

    Legacy migration files store ``python_type`` names; Rust requires
    ``column_type``. Derivation uses the same single mapping module as live
    models. Support for the legacy form is deprecated and will become an
    error in 1.0 (run ``oxyde migrations squash`` to convert).
    """
    if "column_type" in field:
        return field

    from oxyde.core.column_types import compute_column_type, spec_from_legacy_name

    db_type = field.get("db_type")
    spec = None
    if db_type:
        # db_type wins for the semantic kind, same as compute_column_type
        spec = compute_column_type(None, db_type)
    if spec is None:
        spec = spec_from_legacy_name(
            str(field.get("python_type", "")),
            max_length=field.get("max_length"),
            max_digits=field.get("max_digits"),
            decimal_places=field.get("decimal_places"),
        )
    normalized = dict(field)
    normalized["column_type"] = spec
    return normalized


def op_uses_legacy_fields(op: dict) -> bool:
    """True if the operation carries any field dict in the legacy form."""
    return any("column_type" not in f for f in _iter_field_dicts(op))


def normalize_op_fields(op: dict) -> dict:
    """Normalize every FieldDef dict inside a migration operation."""
    op = dict(op)
    table = op.get("table")
    if isinstance(table, dict) and "fields" in table:
        table = dict(table)
        table["fields"] = [normalize_field_dict(f) for f in table["fields"]]
        op["table"] = table
    for key in ("field", "field_def", "old_field", "new_field"):
        if isinstance(op.get(key), dict):
            op[key] = normalize_field_dict(op[key])
    if isinstance(op.get("table_fields"), list):
        op["table_fields"] = [normalize_field_dict(f) for f in op["table_fields"]]
    return op


def _iter_field_dicts(op: dict) -> Iterator[dict]:
    table = op.get("table")
    if isinstance(table, dict):
        yield from table.get("fields", [])
    for key in ("field", "field_def", "old_field", "new_field"):
        if isinstance(op.get(key), dict):
            yield op[key]
    yield from op.get("table_fields") or []

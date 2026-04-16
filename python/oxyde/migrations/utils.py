"""Shared utilities for the migration system."""

from __future__ import annotations

import importlib.util
import sys
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

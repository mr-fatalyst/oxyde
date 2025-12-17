"""Oxyde migrations system.

This module provides database migration functionality:
- Schema extraction from models
- Migration replay in memory
- MigrationContext for upgrade/downgrade operations
- Migration file generation
- Migration tracking and execution
"""

from __future__ import annotations

from oxyde.migrations.context import MigrationContext
from oxyde.migrations.executor import (
    apply_migrations,
    rollback_migration,
    rollback_migrations,
)
from oxyde.migrations.extract import extract_current_schema
from oxyde.migrations.generator import generate_migration_file
from oxyde.migrations.replay import SchemaState, replay_migrations
from oxyde.migrations.tracker import (
    ensure_migrations_table,
    get_applied_migrations,
    get_migration_files,
    get_pending_migrations,
    record_migration,
    remove_migration,
)
from oxyde.migrations.types import (
    normalize_sql_type,
    translate_db_specific_type,
    validate_sql_type,
)

__all__ = [
    "extract_current_schema",
    "SchemaState",
    "replay_migrations",
    "MigrationContext",
    "generate_migration_file",
    "apply_migrations",
    "rollback_migration",
    "rollback_migrations",
    "ensure_migrations_table",
    "get_applied_migrations",
    "get_migration_files",
    "get_pending_migrations",
    "record_migration",
    "remove_migration",
    "validate_sql_type",
    "normalize_sql_type",
    "translate_db_specific_type",
]

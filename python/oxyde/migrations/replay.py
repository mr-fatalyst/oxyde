"""Replay migrations in memory to build schema state."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from typing import Any


class SchemaState:
    """Represents database schema in memory.

    This class applies migration operations to build a virtual schema
    without touching the actual database.
    """

    def __init__(self) -> None:
        self.tables: dict[str, dict[str, Any]] = {}

    def apply_operation(self, op: dict[str, Any]) -> None:
        """Apply a migration operation to the virtual schema.

        Args:
            op: Operation dictionary from MigrationContext.get_collected_operations()
        """
        op_type = op.get("type")

        if op_type == "create_table":
            table = op["table"]
            self.tables[table["name"]] = {
                "name": table["name"],
                "fields": list(table["fields"]),
                "indexes": list(table.get("indexes", [])),
                "foreign_keys": list(table.get("foreign_keys", [])),
                "checks": list(table.get("checks", [])),
                "comment": table.get("comment"),
            }

        elif op_type == "drop_table":
            self.tables.pop(op["name"], None)

        elif op_type == "rename_table":
            old_name = op["old_name"]
            new_name = op["new_name"]
            if old_name in self.tables:
                table = self.tables.pop(old_name)
                table["name"] = new_name
                self.tables[new_name] = table

        elif op_type == "add_column":
            table_name = op["table"]
            if table_name in self.tables:
                self.tables[table_name]["fields"].append(dict(op["field"]))

        elif op_type == "drop_column":
            table_name = op["table"]
            field_name = op["field"]
            if table_name in self.tables:
                self.tables[table_name]["fields"] = [
                    f
                    for f in self.tables[table_name]["fields"]
                    if f["name"] != field_name
                ]

        elif op_type == "rename_column":
            table_name = op["table"]
            old_name = op["old_name"]
            new_name = op["new_name"]
            if table_name in self.tables:
                for field in self.tables[table_name]["fields"]:
                    if field["name"] == old_name:
                        field["name"] = new_name
                        break

        elif op_type == "alter_column":
            table_name = op["table"]
            column_name = op["column"]
            changes = op.get("changes", {})
            if table_name in self.tables:
                for field in self.tables[table_name]["fields"]:
                    if field["name"] == column_name:
                        # Map changes keys to field keys
                        if "type" in changes:
                            field["field_type"] = changes["type"]
                        if "python_type" in changes:
                            field["python_type"] = changes["python_type"]
                        if "db_type" in changes:
                            field["db_type"] = changes["db_type"]
                        if "nullable" in changes:
                            field["nullable"] = changes["nullable"]
                        if "default" in changes:
                            field["default"] = changes["default"]
                        if "unique" in changes:
                            field["unique"] = changes["unique"]
                        break

        elif op_type == "create_index":
            table_name = op["table"]
            if table_name in self.tables:
                self.tables[table_name]["indexes"].append(dict(op["index"]))

        elif op_type == "drop_index":
            table_name = op["table"]
            index_name = op["name"]
            if table_name in self.tables:
                self.tables[table_name]["indexes"] = [
                    idx
                    for idx in self.tables[table_name]["indexes"]
                    if idx["name"] != index_name
                ]

        elif op_type == "add_foreign_key":
            table_name = op["table"]
            if table_name in self.tables:
                if "foreign_keys" not in self.tables[table_name]:
                    self.tables[table_name]["foreign_keys"] = []
                self.tables[table_name]["foreign_keys"].append(dict(op["fk"]))

        elif op_type == "drop_foreign_key":
            table_name = op["table"]
            fk_name = op["name"]
            if table_name in self.tables and "foreign_keys" in self.tables[table_name]:
                self.tables[table_name]["foreign_keys"] = [
                    fk
                    for fk in self.tables[table_name]["foreign_keys"]
                    if fk["name"] != fk_name
                ]

        elif op_type == "add_check":
            table_name = op["table"]
            if table_name in self.tables:
                if "checks" not in self.tables[table_name]:
                    self.tables[table_name]["checks"] = []
                self.tables[table_name]["checks"].append(dict(op["check"]))

        elif op_type == "drop_check":
            table_name = op["table"]
            check_name = op["name"]
            if table_name in self.tables and "checks" in self.tables[table_name]:
                self.tables[table_name]["checks"] = [
                    c
                    for c in self.tables[table_name]["checks"]
                    if c["name"] != check_name
                ]

        # Note: ctx.execute() and Python code are ignored - they don't affect schema

    def to_snapshot(self) -> dict[str, Any]:
        """Convert to Rust-compatible snapshot format.

        Returns:
            Snapshot dictionary compatible with Rust Snapshot structure
        """
        return {
            "version": 1,
            "tables": self.tables,
        }


def _load_migration_module(file: Path) -> Any:
    """Load a migration module from file.

    Args:
        file: Path to migration file

    Returns:
        Loaded module or None if loading failed
    """
    spec = importlib.util.spec_from_file_location(file.stem, file)
    if spec is None or spec.loader is None:
        return None

    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _get_migration_dependency(module: Any) -> str | None:
    """Get the depends_on value from a migration module.

    Args:
        module: Loaded migration module

    Returns:
        Name of dependency migration or None
    """
    return getattr(module, "depends_on", None)


def _topological_sort_migrations(
    migration_files: list[Path],
) -> list[Path]:
    """Sort migrations in topological order based on depends_on.

    Args:
        migration_files: List of migration file paths

    Returns:
        Migrations sorted in dependency order

    Raises:
        ValueError: If circular dependency detected
    """
    # Build dependency graph
    modules: dict[str, tuple[Path, Any]] = {}
    for file in migration_files:
        module = _load_migration_module(file)
        if module is not None:
            modules[file.stem] = (file, module)

    # Build adjacency list (name -> depends_on)
    dependencies: dict[str, str | None] = {}
    for name, (_, module) in modules.items():
        dependencies[name] = _get_migration_dependency(module)

    # Topological sort using Kahn's algorithm
    # Count incoming edges
    in_degree: dict[str, int] = {name: 0 for name in modules}
    for name, dep in dependencies.items():
        if dep is not None and dep in in_degree:
            in_degree[name] = 1  # Has one dependency

    # Start with nodes that have no dependencies
    queue = [name for name, degree in in_degree.items() if degree == 0]
    result: list[Path] = []

    while queue:
        # Sort queue to ensure deterministic order (by migration name)
        queue.sort()
        current = queue.pop(0)
        result.append(modules[current][0])

        # Find migrations that depend on current
        for name, dep in dependencies.items():
            if dep == current:
                in_degree[name] -= 1
                if in_degree[name] == 0:
                    queue.append(name)

    # Check for circular dependencies
    if len(result) != len(modules):
        applied = {f.stem for f in result}
        remaining = set(modules.keys()) - applied
        raise ValueError(f"Circular dependency detected in migrations: {remaining}")

    return result


def replay_migrations(migrations_dir: str = "migrations") -> dict[str, Any]:
    """Replay all migrations and return final schema as snapshot.

    Migrations are applied in topological order based on their depends_on field.

    Args:
        migrations_dir: Directory containing migration files

    Returns:
        Schema snapshot after replaying all migrations
    """
    # Lazy import to avoid circular dependency
    from oxyde.migrations.context import MigrationContext

    state = SchemaState()
    migration_files = sorted(Path(migrations_dir).glob("[0-9]*.py"))

    # Sort migrations by dependencies
    sorted_files = _topological_sort_migrations(migration_files)

    for file in sorted_files:
        module = _load_migration_module(file)
        if module is None:
            continue

        # Run upgrade() in "collect" mode
        if hasattr(module, "upgrade"):
            ctx = MigrationContext(mode="collect")
            module.upgrade(ctx)

            # Apply collected operations to virtual schema
            for op in ctx.get_collected_operations():
                state.apply_operation(op)

    return state.to_snapshot()


def get_migration_order(migrations_dir: str = "migrations") -> list[str]:
    """Get migrations in topological order.

    Args:
        migrations_dir: Directory containing migration files

    Returns:
        List of migration names in dependency order
    """
    migration_files = sorted(Path(migrations_dir).glob("[0-9]*.py"))
    sorted_files = _topological_sort_migrations(migration_files)
    return [f.stem for f in sorted_files]


__all__ = ["SchemaState", "replay_migrations", "get_migration_order"]

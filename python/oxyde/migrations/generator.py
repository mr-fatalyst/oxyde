"""Generate Python migration files from operations."""

from __future__ import annotations

from datetime import datetime
from pathlib import Path
from typing import Any


def _python_repr(obj: Any, indent: int = 0, base_indent: int = 4) -> str:
    """Convert Python object to proper Python repr string.

    Args:
        obj: Object to convert
        indent: Current indentation level (in spaces)
        base_indent: Base indentation increment

    Returns:
        Python code string
    """
    if isinstance(obj, dict):
        if not obj:
            return "{}"

        items = []
        for key, value in obj.items():
            key_repr = repr(key) if isinstance(key, str) else str(key)
            value_repr = _python_repr(value, indent + base_indent, base_indent)
            items.append(f"{' ' * (indent + base_indent)}{key_repr}: {value_repr}")

        return "{\n" + ",\n".join(items) + f"\n{' ' * indent}" + "}"

    elif isinstance(obj, list):
        if not obj:
            return "[]"

        items = []
        for item in obj:
            item_repr = _python_repr(item, indent + base_indent, base_indent)
            items.append(f"{' ' * (indent + base_indent)}{item_repr}")

        return "[\n" + ",\n".join(items) + f"\n{' ' * indent}" + "]"

    elif isinstance(obj, str):
        return repr(obj)

    elif isinstance(obj, bool):
        return "True" if obj else "False"

    elif obj is None:
        return "None"

    else:
        return repr(obj)


def _operation_to_python(op: dict[str, Any], indent: str = "    ") -> str:
    """Convert a single operation to Python code.

    Args:
        op: Operation dictionary
        indent: Indentation string

    Returns:
        Python code string for this operation
    """
    op_type = op.get("type")

    if op_type == "create_table":
        table = op["table"]
        tname = table["name"]
        # Use consistent indentation for all kwargs
        inner_indent = len(indent) + 4

        lines = [f"{indent}ctx.create_table(", f'{indent}    "{tname}",']

        fields_repr = _python_repr(table["fields"], indent=inner_indent)
        lines.append(f"{indent}    fields={fields_repr},")

        if indexes := table.get("indexes", []):
            indexes_repr = _python_repr(indexes, indent=inner_indent)
            lines.append(f"{indent}    indexes={indexes_repr},")

        if foreign_keys := table.get("foreign_keys", []):
            fk_repr = _python_repr(foreign_keys, indent=inner_indent)
            lines.append(f"{indent}    foreign_keys={fk_repr},")

        if checks := table.get("checks", []):
            checks_repr = _python_repr(checks, indent=inner_indent)
            lines.append(f"{indent}    checks={checks_repr},")

        lines.append(f"{indent})")
        return "\n".join(lines)

    elif op_type == "drop_table":
        return f'{indent}ctx.drop_table("{op["name"]}")'

    elif op_type == "rename_table":
        return f'{indent}ctx.rename_table("{op["old_name"]}", "{op["new_name"]}")'

    elif op_type == "add_column":
        field_repr = _python_repr(op["field"])
        return f'{indent}ctx.add_column("{op["table"]}", {field_repr})'

    elif op_type == "drop_column":
        return f'{indent}ctx.drop_column("{op["table"]}", "{op["field"]}")'

    elif op_type == "rename_column":
        t, old, new = op["table"], op["old_name"], op["new_name"]
        return f'{indent}ctx.rename_column("{t}", "{old}", "{new}")'

    elif op_type == "alter_column":
        # Handle both formats:
        # 1. Rust format: old_field/new_field FieldDef dicts
        # 2. Python format: column + changes dict
        if "old_field" in op and "new_field" in op:
            # Rust format - extract column and build changes
            old_field = op["old_field"]
            new_field = op["new_field"]
            column = new_field["name"]
            changes = {}

            # Compare fields and build changes dict
            if old_field.get("field_type") != new_field.get("field_type"):
                changes["type"] = new_field["field_type"]
            if old_field.get("python_type") != new_field.get("python_type"):
                changes["python_type"] = new_field.get("python_type")
            if old_field.get("db_type") != new_field.get("db_type"):
                changes["db_type"] = new_field.get("db_type")
            if old_field.get("nullable") != new_field.get("nullable"):
                changes["nullable"] = new_field["nullable"]
            if old_field.get("default") != new_field.get("default"):
                changes["default"] = new_field.get("default")
            if old_field.get("unique") != new_field.get("unique"):
                changes["unique"] = new_field["unique"]
        else:
            # Python format - use as-is
            column = op["column"]
            changes = op.get("changes", {})

        ch_repr = ", ".join(f"{k}={_python_repr(v)}" for k, v in changes.items())
        return f'{indent}ctx.alter_column("{op["table"]}", "{column}", {ch_repr})'

    elif op_type == "create_index":
        index_repr = _python_repr(op["index"])
        return f'{indent}ctx.create_index("{op["table"]}", {index_repr})'

    elif op_type == "drop_index":
        return f'{indent}ctx.drop_index("{op["table"]}", "{op["name"]}")'

    elif op_type == "add_foreign_key":
        fk = op["fk"]
        columns_repr = repr(fk["columns"])
        ref_columns_repr = repr(fk["ref_columns"])
        on_delete = fk.get("on_delete", "NO ACTION")
        on_update = fk.get("on_update", "NO ACTION")
        return (
            f"{indent}ctx.add_foreign_key(\n"
            f'{indent}    "{op["table"]}",\n'
            f'{indent}    "{fk["name"]}",\n'
            f"{indent}    {columns_repr},\n"
            f'{indent}    "{fk["ref_table"]}",\n'
            f"{indent}    {ref_columns_repr},\n"
            f'{indent}    on_delete="{on_delete}",\n'
            f'{indent}    on_update="{on_update}",\n'
            f"{indent})"
        )

    elif op_type == "drop_foreign_key":
        return f'{indent}ctx.drop_foreign_key("{op["table"]}", "{op["name"]}")'

    elif op_type == "add_check":
        check = op["check"]
        # Escape quotes in expression
        expr = check["expression"].replace('"', '\\"')
        return f'{indent}ctx.add_check("{op["table"]}", "{check["name"]}", "{expr}")'

    elif op_type == "drop_check":
        return f'{indent}ctx.drop_check("{op["table"]}", "{op["name"]}")'

    return f"{indent}# Unknown operation: {op_type}"


def _infer_migration_name(operations: list[dict[str, Any]]) -> str:
    """Infer migration name from operations.

    Args:
        operations: List of migration operations

    Returns:
        Migration name (e.g., "create_user_table")
    """
    if not operations:
        return "empty"

    # Take first operation as basis
    first_op = operations[0]
    op_type = first_op.get("type")

    if op_type == "create_table":
        table_name = first_op["table"]["name"]
        return f"create_{table_name}_table"
    elif op_type == "drop_table":
        return f"drop_{first_op['name']}_table"
    elif op_type == "add_column":
        return f"add_{first_op['field']['name']}_to_{first_op['table']}"
    elif op_type == "drop_column":
        return f"drop_{first_op['field']}_from_{first_op['table']}"
    elif op_type == "rename_table":
        return f"rename_{first_op['old_name']}_to_{first_op['new_name']}"
    elif op_type == "rename_column":
        return f"rename_column_{first_op['old_name']}_to_{first_op['new_name']}"

    # Multiple operations - use generic name
    if len(operations) > 1:
        return "auto"

    return "migration"


def _get_next_migration_number(migrations_dir: str | Path) -> str:
    """Get next migration number.

    Args:
        migrations_dir: Path to migrations directory

    Returns:
        Next migration number as 4-digit string (e.g., "0001")
    """
    migrations_path = Path(migrations_dir)
    if not migrations_path.exists():
        return "0001"

    # Find existing migrations
    existing = sorted(migrations_path.glob("[0-9]*.py"))
    if not existing:
        return "0001"

    # Extract number from last migration
    last_file = existing[-1].stem
    try:
        last_num = int(last_file.split("_")[0])
        return f"{last_num + 1:04d}"
    except (ValueError, IndexError):
        return "0001"


def _get_previous_migration(migrations_dir: str | Path) -> str | None:
    """Get the name of the previous (latest) migration.

    Args:
        migrations_dir: Path to migrations directory

    Returns:
        Name of the previous migration (without .py) or None if no migrations exist
    """
    migrations_path = Path(migrations_dir)
    if not migrations_path.exists():
        return None

    existing = sorted(migrations_path.glob("[0-9]*.py"))
    if not existing:
        return None

    return existing[-1].stem


def generate_migration_file(
    operations: list[dict[str, Any]],
    migrations_dir: str | Path = "migrations",
    name: str | None = None,
) -> Path:
    """Generate a Python migration file.

    Args:
        operations: List of migration operations (from compute_diff)
        migrations_dir: Directory to write migration file
        name: Optional migration name (auto-inferred if not provided)

    Returns:
        Path to generated migration file
    """
    migrations_path = Path(migrations_dir)
    migrations_path.mkdir(parents=True, exist_ok=True)

    # Determine migration name and number
    migration_name = name or _infer_migration_name(operations)
    migration_number = _get_next_migration_number(migrations_path)
    filename = f"{migration_number}_{migration_name}.py"
    filepath = migrations_path / filename

    # Generate upgrade code
    if operations:
        upgrade_lines = [_operation_to_python(op) for op in operations]
        upgrade_body = "\n".join(upgrade_lines)
    else:
        upgrade_body = "    pass  # No operations"

    # Generate downgrade code (reverse operations)
    downgrade_lines = []
    for op in reversed(operations):
        op_type = op.get("type")

        # Generate reverse operation
        if op_type == "create_table":
            downgrade_lines.append(f'    ctx.drop_table("{op["table"]["name"]}")')
        elif op_type == "drop_table":
            # Reverse drop_table by recreating the table from stored structure
            table_def = op.get("table")
            if table_def:
                fields_repr = _python_repr(table_def["fields"], indent=8 + 11)
                indexes = table_def.get("indexes", [])
                tname = table_def["name"]
                if indexes:
                    indexes_repr = _python_repr(indexes, indent=8 + 13)
                    downgrade_lines.append(
                        f'    ctx.create_table(\n        "{tname}",\n'
                        f"        fields={fields_repr},\n"
                        f"        indexes={indexes_repr},\n    )"
                    )
                else:
                    downgrade_lines.append(
                        f'    ctx.create_table(\n        "{tname}",\n'
                        f"        fields={fields_repr},\n    )"
                    )
            else:
                downgrade_lines.append(
                    f"    # TODO: Reverse drop_table for {op['name']}"
                )
        elif op_type == "add_column":
            downgrade_lines.append(
                f'    ctx.drop_column("{op["table"]}", "{op["field"]["name"]}")'
            )
        elif op_type == "drop_column":
            # Reverse drop_column by adding the column back from stored definition
            field_def = op.get("field_def")
            if field_def:
                field_repr = _python_repr(field_def)
                downgrade_lines.append(
                    f'    ctx.add_column("{op["table"]}", {field_repr})'
                )
            else:
                downgrade_lines.append(
                    f"    # TODO: Reverse drop_column for {op['table']}.{op['field']}"
                )
        elif op_type == "rename_table":
            downgrade_lines.append(
                f'    ctx.rename_table("{op["new_name"]}", "{op["old_name"]}")'
            )
        elif op_type == "rename_column":
            t, new, old = op["table"], op["new_name"], op["old_name"]
            downgrade_lines.append(f'    ctx.rename_column("{t}", "{new}", "{old}")')
        elif op_type == "create_index":
            downgrade_lines.append(
                f'    ctx.drop_index("{op["table"]}", "{op["index"]["name"]}")'
            )
        elif op_type == "drop_index":
            # Reverse drop_index by recreating the index from stored definition
            index_def = op.get("index_def")
            if index_def:
                index_repr = _python_repr(index_def)
                downgrade_lines.append(
                    f'    ctx.create_index("{op["table"]}", {index_repr})'
                )
            else:
                downgrade_lines.append(
                    f"    # TODO: Reverse drop_index for {op['table']}.{op['index']}"
                )
        elif op_type == "add_foreign_key":
            downgrade_lines.append(
                f'    ctx.drop_foreign_key("{op["table"]}", "{op["fk"]["name"]}")'
            )
        elif op_type == "drop_foreign_key":
            # Reverse drop_foreign_key by recreating the FK from stored definition
            fk_def = op.get("fk_def")
            if fk_def:
                columns_repr = repr(fk_def["columns"])
                ref_columns_repr = repr(fk_def["ref_columns"])
                on_delete = fk_def.get("on_delete", "NO ACTION")
                on_update = fk_def.get("on_update", "NO ACTION")
                downgrade_lines.append(
                    f"    ctx.add_foreign_key(\n"
                    f'        "{op["table"]}",\n'
                    f'        "{fk_def["name"]}",\n'
                    f"        {columns_repr},\n"
                    f'        "{fk_def["ref_table"]}",\n'
                    f"        {ref_columns_repr},\n"
                    f'        on_delete="{on_delete}",\n'
                    f'        on_update="{on_update}",\n'
                    f"    )"
                )
            else:
                t, n = op["table"], op["name"]
                downgrade_lines.append(
                    f"    # TODO: Reverse drop_foreign_key for {t}.{n}"
                )
        elif op_type == "add_check":
            downgrade_lines.append(
                f'    ctx.drop_check("{op["table"]}", "{op["check"]["name"]}")'
            )
        elif op_type == "drop_check":
            # Reverse drop_check by recreating the check from stored definition
            check_def = op.get("check_def")
            if check_def:
                # Escape quotes in expression
                expr = check_def["expression"].replace('"', '\\"')
                t, name = op["table"], check_def["name"]
                downgrade_lines.append(f'    ctx.add_check("{t}", "{name}", "{expr}")')
            else:
                downgrade_lines.append(
                    f"    # TODO: Reverse drop_check for {op['table']}.{op['name']}"
                )

    downgrade_body = "\n".join(downgrade_lines) if downgrade_lines else "    pass"

    # Get previous migration for dependency
    previous_migration = _get_previous_migration(migrations_path)
    depends_on_line = (
        f'depends_on = "{previous_migration}"'
        if previous_migration
        else "depends_on = None"
    )

    # Generate file content
    content = f'''"""Auto-generated migration.

Created: {datetime.now().strftime("%Y-%m-%d %H:%M:%S")}
"""

{depends_on_line}


def upgrade(ctx):
    """Apply migration."""
{upgrade_body}


def downgrade(ctx):
    """Revert migration."""
{downgrade_body}
'''

    # Write file
    filepath.write_text(content)

    return filepath


__all__ = ["generate_migration_file"]

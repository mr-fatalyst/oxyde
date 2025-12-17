"""Extract database schema from registered models."""

from __future__ import annotations

from datetime import date, datetime, time, timedelta
from decimal import Decimal
from typing import Any
from uuid import UUID

from pydantic.fields import PydanticUndefined

from oxyde.models.registry import iter_tables, registered_tables


def _resolve_fk_target(
    target_key: str, target_field: str | None = None
) -> tuple[str, str]:
    """Resolve FK target to (table_name, column_name).

    Args:
        target_key: Model key like "__main__.User" or table name for primitive FK
        target_field: Target field name (for model FK) or column name
                      (for primitive FK). If None, auto-detects PK.

    Returns:
        Tuple of (table_name, column_name)
    """
    # Get all registered tables
    tables = registered_tables()

    # Try to find by full key or class name
    for key, model_cls in tables.items():
        if key == target_key or model_cls.__name__ == target_key.split(".")[-1]:
            model_cls.ensure_field_metadata()
            table_name = model_cls._db_meta.table_name or model_cls.__name__.lower()

            if target_field:
                # Look up the specified field's db_column
                for field_name, field_meta in model_cls._db_meta.field_metadata.items():
                    if field_name == target_field:
                        return table_name, field_meta.db_column
                # Field not found in metadata - use target_field as column name
                return table_name, target_field
            else:
                # Find PK column (fallback for backwards compatibility)
                pk_column = "id"  # default
                for field_name, field_meta in model_cls._db_meta.field_metadata.items():
                    if field_meta.primary_key:
                        pk_column = field_meta.db_column
                        break
                return table_name, pk_column

    # No matching model found - target_key is likely a plain table name (primitive FK)
    # Use target_key as table name and target_field as column name
    if target_field:
        return target_key, target_field

    # Fallback: extract table name from key, assume "id" as column
    class_name = target_key.split(".")[-1]
    return class_name.lower(), "id"


def _serialize_default(value: Any, dialect: str) -> str | None:
    """Serialize Python default value to SQL default expression.

    Args:
        value: Python default value
        dialect: Database dialect

    Returns:
        SQL default expression string, or None if cannot serialize
    """
    if value is None:
        return "NULL"

    # Callable defaults (like datetime.now) cannot be serialized
    # User should use db_default for SQL expressions
    if callable(value):
        return None

    # String - quote it
    if isinstance(value, str):
        # Escape single quotes
        escaped = value.replace("'", "''")
        return f"'{escaped}'"

    # Boolean
    if isinstance(value, bool):
        if dialect == "postgres":
            return "TRUE" if value else "FALSE"
        else:
            return "1" if value else "0"

    # Numbers
    if isinstance(value, (int, float, Decimal)):
        return str(value)

    # UUID
    if isinstance(value, UUID):
        return f"'{value}'"

    # Date/time types
    if isinstance(value, datetime):
        return f"'{value.isoformat()}'"
    if isinstance(value, date):
        return f"'{value.isoformat()}'"
    if isinstance(value, time):
        return f"'{value.isoformat()}'"

    # Bytes
    if isinstance(value, bytes):
        if dialect == "postgres":
            return f"'\\x{value.hex()}'"
        elif dialect == "sqlite":
            return f"X'{value.hex()}'"
        else:  # mysql
            return f"X'{value.hex()}'"

    # Fallback - try str() but return None if it looks like a repr
    str_val = str(value)
    if str_val.startswith("<") or "object at" in str_val:
        return None
    return str_val


def _get_python_type_name(python_type: type) -> str:
    """Get canonical name for Python type.

    Used for cross-dialect type generation in migrations.

    Args:
        python_type: Python type (int, str, bytes, etc.)

    Returns:
        Canonical type name string
    """
    # Map types to canonical names
    type_names = {
        int: "int",
        str: "str",
        float: "float",
        bool: "bool",
        bytes: "bytes",
        datetime: "datetime",
        date: "date",
        time: "time",
        timedelta: "timedelta",
        UUID: "uuid",
        Decimal: "decimal",
    }

    if python_type in type_names:
        return type_names[python_type]

    # For dict, list, etc. - use the type name
    return python_type.__name__.lower()


def extract_current_schema(dialect: str = "sqlite") -> dict[str, Any]:
    """Extract current schema from registered models.

    Args:
        dialect: Target database dialect (sqlite, postgres, mysql)

    Returns:
        Schema snapshot compatible with Rust Snapshot structure:
        {
            "version": 1,
            "tables": {
                "table_name": {
                    "name": "table_name",
                    "fields": [...],
                    "indexes": [...],
                    "comment": None
                }
            }
        }
    """
    # Import here to avoid circular dependency
    from oxyde.models.base import OxydeModel

    tables = {}

    for model_cls in iter_tables():
        # Ensure field metadata is populated
        model_cls.ensure_field_metadata()
        meta = model_cls._db_meta

        # Extract fields
        fields = []
        for field_name, field_meta in meta.field_metadata.items():
            # Skip virtual relation fields - these are for ORM, not actual DB columns:
            # 1. FK fields (python_type is OxydeModel) - real column is synthetic field
            # 2. Reverse FK fields (db_reverse_fk) - no DB column, just relation
            # 3. M2M fields (db_m2m) - no DB column, uses junction table
            if isinstance(field_meta.python_type, type) and issubclass(
                field_meta.python_type, OxydeModel
            ):
                continue
            if field_meta.extra.get("reverse_fk") or field_meta.extra.get("m2m"):
                continue

            # Determine Python type for SQL mapping
            # For FK fields, use int (references PK which is typically int)
            if field_meta.foreign_key is not None:
                # FK columns store the PK value of the referenced table
                # Most PKs are int, so FK columns should be int type
                python_type_for_sql = int
            else:
                python_type_for_sql = field_meta.python_type

            # Determine if this field needs AUTO_INCREMENT (MySQL only)
            # AUTO_INCREMENT is needed when:
            # 1. It's a primary key AND python type is int AND
            # 2. Either db_type is not specified OR db_type is SERIAL/BIGSERIAL
            auto_increment = False
            if (
                dialect == "mysql"
                and field_meta.primary_key
                and field_meta.python_type is int
            ):
                if field_meta.db_type is None:
                    # No explicit db_type - auto-increment enabled
                    auto_increment = True
                elif field_meta.db_type.upper() in ("SERIAL", "BIGSERIAL"):
                    # Explicit SERIAL/BIGSERIAL - auto-increment enabled
                    auto_increment = True

            # Convert default value to SQL expression
            default_val = None
            # Priority: db_default (explicit SQL) > default (Python value)
            if hasattr(field_meta, "db_default") and field_meta.db_default is not None:
                # db_default is a raw SQL expression - use as-is
                default_val = field_meta.db_default
            elif (
                field_meta.default is not None
                and field_meta.default is not PydanticUndefined
            ):
                # Serialize Python value to SQL
                default_val = _serialize_default(field_meta.default, dialect)

            # Get python type name for cross-dialect type generation
            python_type_name = _get_python_type_name(python_type_for_sql)

            fields.append(
                {
                    "name": field_meta.db_column,
                    "python_type": python_type_name,
                    "db_type": field_meta.db_type,  # explicit user override
                    "nullable": field_meta.nullable,
                    "primary_key": field_meta.primary_key,
                    "unique": field_meta.unique,
                    "default": default_val,
                    "auto_increment": auto_increment,
                }
            )

        # Extract indexes
        indexes = []
        if hasattr(meta, "indexes") and meta.indexes:
            for index in meta.indexes:
                # Generate index name if not provided
                index_name = index.name
                if not index_name and index.fields:
                    index_name = f"{meta.table_name}_{'_'.join(index.fields)}_idx"

                indexes.append(
                    {
                        "name": index_name,
                        "fields": list(index.fields),
                        "unique": index.unique,
                        "method": index.method,
                    }
                )

        # Extract foreign keys from field metadata
        foreign_keys = []
        for field_name, field_meta in meta.field_metadata.items():
            if field_meta.foreign_key:
                fk = field_meta.foreign_key
                # Resolve target model to actual table name and column
                ref_table, ref_column = _resolve_fk_target(fk.target, fk.target_field)
                # Generate FK constraint name
                fk_name = f"fk_{meta.table_name}_{field_meta.db_column}"
                foreign_keys.append(
                    {
                        "name": fk_name,
                        "columns": [field_meta.db_column],
                        "ref_table": ref_table,
                        "ref_columns": [ref_column],
                        "on_delete": fk.on_delete,
                        "on_update": fk.on_update,
                    }
                )

        # Extract check constraints from Meta.constraints
        checks = []
        if hasattr(meta, "constraints") and meta.constraints:
            for i, check in enumerate(meta.constraints):
                # Check object has .expression and .name attributes
                check_name = check.name or f"chk_{meta.table_name}_{i}"
                checks.append(
                    {
                        "name": check_name,
                        "expression": check.expression,
                    }
                )

        # Build table definition
        tables[meta.table_name] = {
            "name": meta.table_name,
            "fields": fields,
            "indexes": indexes,
            "foreign_keys": foreign_keys,
            "checks": checks,
            "comment": meta.comment,
        }

    return {
        "version": 1,
        "tables": tables,
    }


__all__ = ["extract_current_schema"]

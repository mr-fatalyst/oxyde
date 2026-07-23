"""The single Python → ColumnTypeSpec mapping point.

Converts a field's Python annotation + user ``db_type`` + constraints into a
``ColumnTypeSpec`` dict — the tagged form the Rust core deserializes
(``{"kind": "decimal", "precision": 10, "scale": 2}``).

This module is the only place in the Python layer that classifies types.
Everything else (model finalization, migrations extract, F-expression
literals) delegates here. ``TYPE_REGISTRY`` keeps its other roles
(msgpack value serialization, lookup categories) untouched.

Two roles of ``db_type`` are split deliberately:
- its *semantic kind* (for binding/decoding) is derived here via
  ``_KNOWN_DB_TYPES``;
- its *verbatim DDL string* travels separately (``FieldDef.db_type``)
  and is never classified in Rust.

Unknown annotations and unrecognized ``db_type`` strings return ``None``:
the column is simply omitted from ``column_types`` and Rust falls back to
native conversion / runtime DB type info — exactly the legacy behavior.
"""

from __future__ import annotations

from datetime import date, datetime, time, timedelta
from decimal import Decimal
from enum import Enum
from typing import Any, get_args, get_origin
from uuid import UUID

ColumnSpec = dict[str, Any]

# Scalar Python type → spec kind (exact type() lookup, no issubclass).
_PY_SCALAR_KINDS: dict[type, str] = {
    bool: "boolean",
    int: "big_integer",
    float: "double",
    str: "string",
    bytes: "blob",
    bytearray: "blob",
    datetime: "date_time",
    date: "date",
    time: "time",
    timedelta: "timedelta",
    UUID: "uuid",
    Decimal: "decimal",
    dict: "json",
}

# Uppercased SQL type name (precision stripped) → spec kind.
# Mirrors the legacy Rust `classify_type` table: only names whose binding
# semantics are known. Anything else → None (native conversion).
_KNOWN_DB_TYPES: dict[str, str] = {
    "INT": "big_integer",
    "INTEGER": "big_integer",
    "BIGINT": "big_integer",
    "SMALLINT": "big_integer",
    "TINYINT": "big_integer",
    "SERIAL": "big_integer",
    "BIGSERIAL": "big_integer",
    "SMALLSERIAL": "big_integer",
    "INT2": "big_integer",
    "INT4": "big_integer",
    "INT8": "big_integer",
    "TIMEDELTA": "timedelta",
    "INTERVAL": "timedelta",
    "FLOAT": "double",
    "DOUBLE": "double",
    "REAL": "double",
    "FLOAT4": "double",
    "FLOAT8": "double",
    "DOUBLE PRECISION": "double",
    "BOOL": "boolean",
    "BOOLEAN": "boolean",
    "TEXT": "text",
    "STR": "text",
    "VARCHAR": "string",
    "CHAR": "string",
    "UUID": "uuid",
    "JSON": "json",
    "JSONB": "json_binary",
    "DATETIME": "date_time",
    "TIMESTAMP": "date_time",
    "TIMESTAMPTZ": "date_time_utc",
    "DATE": "date",
    "TIME": "time",
    "TIMETZ": "time",
    "BYTES": "blob",
    "BYTEA": "blob",
    "BLOB": "blob",
    "DECIMAL": "decimal",
    "NUMERIC": "decimal",
}


def compute_column_type(
    python_type: Any,
    db_type: str | None = None,
    *,
    max_length: int | None = None,
    max_digits: int | None = None,
    decimal_places: int | None = None,
) -> ColumnSpec | None:
    """Compute the ColumnTypeSpec dict for a column.

    Priority: explicit ``db_type`` (semantic kind via _KNOWN_DB_TYPES,
    parameters parsed from e.g. ``NUMERIC(10,2)``/``VARCHAR(100)``),
    otherwise the Python annotation. Returns None when nothing is known —
    the column is then omitted from ``column_types``.
    """
    enum_spec = _spec_from_enum_annotation(python_type, db_type)
    if enum_spec is not None:
        return enum_spec
    if db_type:
        return _spec_from_db_type(db_type)
    return _spec_from_annotation(
        python_type,
        max_length=max_length,
        max_digits=max_digits,
        decimal_places=decimal_places,
    )


def spec_for_literal(value_type: type) -> ColumnSpec | None:
    """Spec for an F-expression literal by its Python type.

    Used to recover typed bindings (Decimal, UUID, datetime, ...) for values
    that msgpack-encode as strings. Returns None for types msgpack carries
    natively with correct binding.
    """
    if _is_enum_type(value_type):
        return _enum_spec(value_type)
    kind = _PY_SCALAR_KINDS.get(value_type)
    return {"kind": kind} if kind is not None else None


def _spec_from_db_type(db_type: str) -> ColumnSpec | None:
    upper = db_type.upper().strip()

    if upper.endswith("[]"):
        item = _spec_from_db_type(upper[:-2])
        return {"kind": "array", "item": item} if item else None

    base, params = _split_type_params(upper)
    kind = _KNOWN_DB_TYPES.get(base)
    if kind is None:
        return None

    spec: ColumnSpec = {"kind": kind}
    if kind == "string" and params:
        spec["length"] = params[0]
    elif kind == "decimal" and params:
        spec["precision"] = params[0]
        if len(params) > 1:
            spec["scale"] = params[1]
    return spec


def _spec_from_annotation(
    python_type: Any,
    *,
    max_length: int | None = None,
    max_digits: int | None = None,
    decimal_places: int | None = None,
) -> ColumnSpec | None:
    origin = get_origin(python_type)

    # dict[K, V] -> json
    if origin is dict:
        return {"kind": "json"}

    # list[T] -> array of T; constraints apply to the element
    # (legacy parity: max_length on a list field meant VARCHAR(n)[] elements)
    if origin is list:
        args = get_args(python_type)
        if args:
            item = _spec_from_annotation(
                args[0],
                max_length=max_length,
                max_digits=max_digits,
                decimal_places=decimal_places,
            )
            if item:
                return {"kind": "array", "item": item}
        return None

    # Union types (including Optional[T]) -> first non-None arg
    if origin is not None:
        for arg in get_args(python_type):
            if arg is not type(None):
                result = _spec_from_annotation(
                    arg,
                    max_length=max_length,
                    max_digits=max_digits,
                    decimal_places=decimal_places,
                )
                if result:
                    return result
        return None

    kind = _PY_SCALAR_KINDS.get(python_type)
    if kind is None:
        if _is_enum_type(python_type):
            return _enum_spec(python_type)
        return None

    spec: ColumnSpec = {"kind": kind}
    if kind == "string" and max_length is not None:
        spec["length"] = max_length
    elif kind == "decimal":
        if max_digits is not None:
            spec["precision"] = max_digits
        if decimal_places is not None:
            spec["scale"] = decimal_places
    return spec


def _spec_from_enum_annotation(
    python_type: Any,
    db_type: str | None,
) -> ColumnSpec | None:
    if db_type:
        # Explicit db_type = plain verbatim column, enum machinery off.
        return None
    enum_info = _enum_annotation_info(python_type)
    if enum_info is None:
        return None
    enum_type, is_array = enum_info
    spec = _enum_spec(enum_type)
    return {"kind": "array", "item": spec} if is_array else spec


def _enum_annotation_info(python_type: Any) -> tuple[type[Enum], bool] | None:
    origin = get_origin(python_type)
    if origin is list:
        args = get_args(python_type)
        if not args:
            return None
        inner = _enum_annotation_info(args[0])
        return (inner[0], True) if inner is not None else None
    if origin is not None:
        for arg in get_args(python_type):
            if arg is type(None):
                continue
            enum_info = _enum_annotation_info(arg)
            if enum_info is not None:
                return enum_info
        return None
    return (python_type, False) if _is_enum_type(python_type) else None


def _is_enum_type(value: Any) -> bool:
    return isinstance(value, type) and issubclass(value, Enum)


def _enum_spec(enum_type: type[Enum]) -> ColumnSpec:
    return {
        "kind": "enum",
        "name": _default_enum_type_name(enum_type),
        "values": _enum_values(enum_type),
    }


def _enum_values(enum_type: type[Enum]) -> list[str]:
    values = []
    for member in enum_type:
        value = member.value
        if not isinstance(value, str):
            raise TypeError(
                f"Enum field '{enum_type.__name__}' must define string values. "
                "Use a str-valued Enum for database enum columns, or annotate "
                "the field as int for integer storage."
            )
        values.append(value)
    return values


def _default_enum_type_name(enum_type: type[Enum]) -> str:
    name = enum_type.__name__
    parts: list[str] = []
    for index, char in enumerate(name):
        if char.isupper() and index > 0:
            prev = name[index - 1]
            next_char = name[index + 1] if index + 1 < len(name) else ""
            if prev != "_" and (not prev.isupper() or next_char.islower()):
                parts.append("_")
        parts.append(char.lower())
    return f"{''.join(parts)}_enum"


def _split_type_params(upper: str) -> tuple[str, list[int]]:
    """Split "NUMERIC(10,2)" → ("NUMERIC", [10, 2]); no parens → (name, [])."""
    if "(" not in upper:
        return upper, []
    base, _, rest = upper.partition("(")
    params: list[int] = []
    for part in rest.rstrip(")").split(","):
        part = part.strip()
        if part.isdigit():
            params.append(int(part))
        else:
            return base.strip(), []
    return base.strip(), params


# Legacy migration-file type names ("int", "str", "decimal", "custom_thing",
# "int[]") → spec kind. Used only by the legacy reader in migrations
# (to_snapshot normalization); becomes a hard error in 1.0.
_LEGACY_NAME_KINDS: dict[str, str] = {
    "int": "big_integer",
    "str": "string",
    "float": "double",
    "bool": "boolean",
    "bytes": "blob",
    "datetime": "date_time",
    "date": "date",
    "time": "time",
    "timedelta": "timedelta",
    "uuid": "uuid",
    "decimal": "decimal",
    "json": "json",
}


def spec_from_legacy_name(
    name: str,
    *,
    max_length: int | None = None,
    max_digits: int | None = None,
    decimal_places: int | None = None,
) -> ColumnSpec:
    """Spec from a legacy migration-file type name.

    Unknown names (custom annotation classes) collapse to ``unknown`` —
    they always rendered as TEXT and bound natively. Never returns None:
    ``FieldDef.column_type`` is required on the Rust side.
    """
    if name.endswith("[]"):
        item = spec_from_legacy_name(
            name[:-2],
            max_length=max_length,
            max_digits=max_digits,
            decimal_places=decimal_places,
        )
        return {"kind": "array", "item": item}

    kind = _LEGACY_NAME_KINDS.get(name)
    if kind is None:
        return {"kind": "unknown"}

    spec: ColumnSpec = {"kind": kind}
    if kind == "string" and max_length is not None:
        spec["length"] = max_length
    elif kind == "decimal":
        if max_digits is not None:
            spec["precision"] = max_digits
        if decimal_places is not None:
            spec["scale"] = decimal_places
    return spec


__all__ = [
    "ColumnSpec",
    "compute_column_type",
    "spec_for_literal",
    "spec_from_legacy_name",
]

"""Django-style field lookups for query filtering.

This module implements the lookup system that powers filter() expressions.
Lookups are suffixes like __gte, __contains, __in that modify comparisons.

Lookup Categories by Field Type:
    String fields:
        exact, iexact, contains, icontains, startswith, istartswith,
        endswith, iendswith, in, isnull

    Numeric fields (int, float, Decimal):
        exact, gt, gte, lt, lte, between, range, in, isnull

    DateTime/Date fields:
        exact, gt, gte, lt, lte, between, range, year, month, day,
        in, isnull

    Boolean fields:
        exact, in, isnull

    All fields:
        exact (default), in, isnull

Key Functions:
    _split_lookup_key(key: str) -> tuple[str, str]:
        Split "name__contains" into ("name", "contains").
        Returns ("name", "exact") if no __ present.

    _build_lookup_conditions(model, field, lookup, value, meta) -> list[Condition]:
        Convert a lookup into Condition objects for the IR.

    _resolve_column_meta(model, field) -> ColumnMeta:
        Get metadata for a field, with fallback for non-table models.

SQL Mapping:
    exact     → = (or IS NULL for None)
    gt/gte    → > / >=
    lt/lte    → < / <=
    in        → IN (...)
    between   → BETWEEN ... AND ...
    isnull    → IS NULL / IS NOT NULL
    contains  → LIKE '%...%'
    icontains → ILIKE '%...%' (case-insensitive)
    startswith → LIKE '...%'
    endswith  → LIKE '%...'
    iexact    → ILIKE '...'
    year/month/day → range conditions on date/datetime

Example:
    User.objects.filter(
        name__icontains="alice",  # ILIKE '%alice%'
        age__gte=18,              # >= 18
        status__in=["active", "pending"],
        created_at__year=2024,    # >= '2024-01-01' AND < '2025-01-01'
    )
"""

from __future__ import annotations

import collections.abc
from dataclasses import dataclass
from datetime import date, datetime, time, timedelta
from decimal import Decimal
from typing import TYPE_CHECKING, Any

from oxyde.exceptions import FieldError, FieldLookupError, FieldLookupValueError
from oxyde.models.metadata import ColumnMeta
from oxyde.models.utils import _unpack_annotated, _unwrap_optional

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel
    from oxyde.queries.conditions import Condition


# Lookup constants
STRING_LOOKUPS = [
    "contains",
    "icontains",
    "startswith",
    "istartswith",
    "endswith",
    "iendswith",
    "iexact",
]
NUMERIC_LOOKUPS = ["gt", "gte", "lt", "lte", "between", "range"]
DATETIME_LOOKUPS = ["gt", "gte", "lt", "lte", "between", "range"]
BOOL_LOOKUPS: list[str] = []
GENERIC_LOOKUPS: list[str] = []
COMMON_LOOKUPS = ["in", "isnull"]
DATE_PART_LOOKUPS = ["year", "month", "day"]

# All known lookups for path parsing
ALL_LOOKUPS = frozenset(
    ["exact"] + STRING_LOOKUPS + NUMERIC_LOOKUPS + COMMON_LOOKUPS + DATE_PART_LOOKUPS
)


def _lookup_category(meta: ColumnMeta) -> str:
    """Determine lookup category based on field metadata."""
    python_type = meta.python_type
    if meta.foreign_key is not None:
        return "numeric"
    if isinstance(python_type, type):
        if issubclass(python_type, str):
            return "string"
        if issubclass(python_type, (int, float, Decimal)):
            return "numeric"
        if issubclass(python_type, (datetime, date, time)):
            return "datetime"
        if issubclass(python_type, bool):
            return "bool"
    return "generic"


def _allowed_lookups_for_meta(meta: ColumnMeta) -> list[str]:
    """Get list of allowed lookups for a given field metadata."""
    category = _lookup_category(meta)
    lookups: list[str] = ["exact"]
    if category == "string":
        lookups.extend(STRING_LOOKUPS)
    elif category == "numeric":
        lookups.extend(NUMERIC_LOOKUPS)
    elif category == "datetime":
        lookups.extend(DATETIME_LOOKUPS)
        lookups.extend(DATE_PART_LOOKUPS)
    elif category == "bool":
        lookups.extend(BOOL_LOOKUPS)
    else:
        lookups.extend(GENERIC_LOOKUPS)
    lookups.extend(COMMON_LOOKUPS)
    # Deduplicate while preserving order
    seen: set[str] = set()
    result: list[str] = []
    for item in lookups:
        if item not in seen:
            seen.add(item)
            result.append(item)
    return result


def _split_lookup_key(key: str) -> tuple[str, str]:
    """Split field__lookup key into field name and lookup type.

    For simple cases like "name__contains", returns ("name", "contains").
    For nested paths like "user__age__gte", use _parse_lookup_path instead.
    """
    if "__" not in key:
        return key, "exact"
    field_name, lookup = key.split("__", 1)
    if not field_name:
        raise FieldLookupError("Lookup key must include a field name before '__'")
    return field_name, lookup


def _parse_lookup_path(key: str) -> tuple[list[str], str]:
    """Parse a lookup key into field path and lookup type.

    Handles nested paths like "user__age__gte" -> (["user", "age"], "gte").

    Args:
        key: Lookup key like "name", "name__contains", or "user__age__gte"

    Returns:
        Tuple of (field_path, lookup_type) where field_path is a list of field names

    Examples:
        "name" -> (["name"], "exact")
        "name__contains" -> (["name"], "contains")
        "user__age" -> (["user", "age"], "exact")
        "user__age__gte" -> (["user", "age"], "gte")
        "user__profile__city__icontains" -> (["user", "profile", "city"], "icontains")
    """
    if "__" not in key:
        return [key], "exact"

    parts = key.split("__")
    if not parts[0]:
        raise FieldLookupError("Lookup key must include a field name before '__'")

    # Check if last part is a known lookup
    if parts[-1] in ALL_LOOKUPS:
        return parts[:-1], parts[-1]

    # No lookup suffix, treat entire path as field path with "exact" lookup
    return parts, "exact"


@dataclass
class ResolvedPath:
    """Result of resolving a field path through FK relationships."""

    # List of (relation_name, target_model) for each FK traversal
    joins: list[tuple[str, type[OxydeModel]]]
    # Final field name (e.g., "age" in "user__age")
    final_field: str
    # Final model class where final_field is defined
    final_model: type[OxydeModel]
    # Column metadata for the final field
    column_meta: ColumnMeta


def _resolve_field_path(
    model_class: type[OxydeModel],
    field_path: list[str],
) -> ResolvedPath:
    """Resolve a field path through FK relationships.

    For "user__age" on Post model:
    1. Post.user is FK to User
    2. User.age is the final field

    Args:
        model_class: Starting model (e.g., Post)
        field_path: List of field names (e.g., ["user", "age"])

    Returns:
        ResolvedPath with joins info and final field metadata

    Raises:
        FieldError: If field doesn't exist
        FieldLookupError: If path traverses non-FK field
    """
    from oxyde.queries.base import _resolve_registered_model

    joins: list[tuple[str, type[OxydeModel]]] = []
    current_model = model_class

    # Traverse all but the last field (those must be FK fields)
    for i, field_name in enumerate(field_path[:-1]):
        current_model.ensure_field_metadata()
        meta = current_model._db_meta.field_metadata.get(field_name)

        if meta is None:
            raise FieldError(f"{current_model.__name__} has no field '{field_name}'")

        if meta.foreign_key is None:
            raise FieldLookupError(
                f"Cannot traverse '{field_name}' in path "
                f"'{'.'.join(field_path)}': not a foreign key"
            )

        # Resolve target model
        target_model = _resolve_registered_model(meta.foreign_key.target)
        joins.append((field_name, target_model))
        current_model = target_model

    # Resolve the final field
    final_field = field_path[-1]
    column_meta = _resolve_column_meta(current_model, final_field)

    return ResolvedPath(
        joins=joins,
        final_field=final_field,
        final_model=current_model,
        column_meta=column_meta,
    )


def _resolve_column_meta(model_class: type[OxydeModel], field_name: str) -> ColumnMeta:
    """Resolve column metadata for a field, creating basic metadata if needed."""
    model_class.ensure_field_metadata()
    meta = model_class._db_meta.field_metadata.get(field_name)
    if meta is not None:
        return meta

    model_field = model_class.model_fields.get(field_name)
    if model_field is None:
        raise FieldError(f"{model_class.__name__} has no field '{field_name}'")

    base_hint, _ = _unpack_annotated(model_field.annotation)
    python_type, optional_flag = _unwrap_optional(base_hint)
    nullable = optional_flag or not model_field.is_required()

    return ColumnMeta(
        name=field_name,
        db_column=field_name,
        python_type=python_type,
        nullable=nullable,
    )


def _build_lookup_conditions(
    model_class: type[OxydeModel],
    field_name: str,
    lookup: str,
    value: Any,
    column_meta: ColumnMeta,
) -> list[Condition]:
    """Build query conditions for a given field lookup."""
    # Import locally to avoid circular dependency
    from oxyde.queries.conditions import Condition

    lookup = lookup or "exact"
    db_column = column_meta.db_column

    if lookup == "exact":
        if value is None:
            return [Condition(field_name, "IS NULL", None, column=db_column)]
        return [Condition(field_name, "=", value, column=db_column)]

    if lookup in {"gt", "gte", "lt", "lte"}:
        if value is None:
            raise FieldLookupValueError(f"Lookup '{lookup}' requires a non-null value")
        op_map = {"gt": ">", "gte": ">=", "lt": "<", "lte": "<="}
        return [Condition(field_name, op_map[lookup], value, column=db_column)]

    if lookup == "in":
        if value is None:
            raise FieldLookupValueError("Lookup 'in' requires an iterable of values")
        if isinstance(value, (str, bytes)):
            raise FieldLookupValueError(
                "Lookup 'in' does not accept string values; use a sequence"
            )
        if not isinstance(value, collections.abc.Iterable):
            raise FieldLookupValueError("Lookup 'in' requires an iterable value")
        return [Condition(field_name, "IN", list(value), column=db_column)]

    if lookup in ("between", "range"):
        if not isinstance(value, (tuple, list)) or len(value) != 2:
            raise FieldLookupValueError(
                f"Lookup '{lookup}' requires a tuple/list of two values"
            )
        return [
            Condition(
                field_name,
                "BETWEEN",
                [value[0], value[1]],
                column=column_meta.db_column,
            )
        ]

    if lookup == "isnull":
        flag = bool(value)
        return [
            Condition(
                field_name,
                "IS NULL" if flag else "IS NOT NULL",
                None,
                column=column_meta.db_column,
            )
        ]

    if lookup in {
        "contains",
        "icontains",
        "startswith",
        "istartswith",
        "endswith",
        "iendswith",
    }:
        if not isinstance(value, str):
            raise FieldLookupValueError(f"Lookup '{lookup}' requires a string value")

        operator = "LIKE"
        if lookup in {"icontains", "istartswith", "iendswith"}:
            operator = "ILIKE"

        # Escape SQL wildcards to treat them as literal characters
        # Order matters: escape backslash first, then wildcards
        escaped_value = (
            value.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
        )

        if lookup.endswith("contains"):
            pattern = f"%{escaped_value}%"
        elif lookup.endswith("startswith"):
            pattern = f"{escaped_value}%"
        else:  # endswith / iendswith
            pattern = f"%{escaped_value}"

        return [Condition(field_name, operator, pattern, column=column_meta.db_column)]

    if lookup == "iexact":
        if not isinstance(value, str):
            raise FieldLookupValueError("Lookup 'iexact' requires a string value")
        return [Condition(field_name, "ILIKE", value, column=column_meta.db_column)]

    if lookup == "year":
        return _build_year_conditions(field_name, value, column_meta)

    if lookup == "month":
        return _build_month_conditions(field_name, value, column_meta)

    if lookup == "day":
        return _build_day_conditions(field_name, value, column_meta)

    raise FieldLookupError(f"Unsupported lookup '{lookup}' for field '{field_name}'")


def _ensure_date_inputs(value: Any, expected: int, label: str) -> tuple[int, ...]:
    """Validate and normalize date/time lookup inputs."""
    if isinstance(value, int) and expected == 1:
        return (value,)
    if isinstance(value, (tuple, list)) and len(value) == expected:
        if all(isinstance(v, int) for v in value):
            return tuple(value)  # type: ignore[return-value]
    raise FieldLookupValueError(f"Lookup '{label}' expects {expected} integer value(s)")


def _build_year_conditions(
    field_name: str, value: Any, meta: ColumnMeta
) -> list[Condition]:
    """Build conditions for year lookup."""
    from oxyde.queries.conditions import Condition

    (year,) = _ensure_date_inputs(value, 1, "year")
    if isinstance(meta.python_type, type) and issubclass(meta.python_type, datetime):
        start = datetime(year, 1, 1)
        end = datetime(year + 1, 1, 1)
    elif isinstance(meta.python_type, type) and issubclass(meta.python_type, date):
        start = date(year, 1, 1)
        end = date(year + 1, 1, 1)
    else:
        raise FieldLookupError(
            f"Lookup 'year' is not supported for field type {meta.python_type}"
        )
    return [
        Condition(field_name, ">=", start.isoformat(), column=meta.db_column),
        Condition(field_name, "<", end.isoformat(), column=meta.db_column),
    ]


def _build_month_conditions(
    field_name: str, value: Any, meta: ColumnMeta
) -> list[Condition]:
    """Build conditions for month lookup."""
    from oxyde.queries.conditions import Condition

    year, month = _ensure_date_inputs(value, 2, "month")
    if not 1 <= month <= 12:
        raise FieldLookupValueError("Lookup 'month' requires month in range 1..12")
    if isinstance(meta.python_type, type) and issubclass(meta.python_type, datetime):
        start = datetime(year, month, 1)
        end_month = month + 1
        end_year = year
        if end_month == 13:
            end_month = 1
            end_year += 1
        end = datetime(end_year, end_month, 1)
    elif isinstance(meta.python_type, type) and issubclass(meta.python_type, date):
        start = date(year, month, 1)
        end_month = month + 1
        end_year = year
        if end_month == 13:
            end_month = 1
            end_year += 1
        end = date(end_year, end_month, 1)
    else:
        raise FieldLookupError(
            f"Lookup 'month' is not supported for field type {meta.python_type}"
        )
    return [
        Condition(field_name, ">=", start.isoformat(), column=meta.db_column),
        Condition(field_name, "<", end.isoformat(), column=meta.db_column),
    ]


def _build_day_conditions(
    field_name: str, value: Any, meta: ColumnMeta
) -> list[Condition]:
    """Build conditions for day lookup."""
    from oxyde.queries.conditions import Condition

    year, month, day = _ensure_date_inputs(value, 3, "day")
    try:
        if isinstance(meta.python_type, type) and issubclass(
            meta.python_type, datetime
        ):
            start = datetime(year, month, day)
            end = start + timedelta(days=1)
        elif isinstance(meta.python_type, type) and issubclass(meta.python_type, date):
            start = date(year, month, day)
            end = start + timedelta(days=1)
        else:
            raise FieldLookupError(
                f"Lookup 'day' is not supported for field type {meta.python_type}"
            )
    except ValueError as exc:  # invalid date
        raise FieldLookupValueError(str(exc)) from exc
    return [
        Condition(field_name, ">=", start.isoformat(), column=meta.db_column),
        Condition(field_name, "<", end.isoformat(), column=meta.db_column),
    ]


__all__ = [
    "STRING_LOOKUPS",
    "NUMERIC_LOOKUPS",
    "DATETIME_LOOKUPS",
    "BOOL_LOOKUPS",
    "GENERIC_LOOKUPS",
    "COMMON_LOOKUPS",
    "DATE_PART_LOOKUPS",
    "ALL_LOOKUPS",
    "ResolvedPath",
    "_lookup_category",
    "_allowed_lookups_for_meta",
    "_split_lookup_key",
    "_parse_lookup_path",
    "_resolve_field_path",
    "_resolve_column_meta",
    "_build_lookup_conditions",
    "_ensure_date_inputs",
    "_build_year_conditions",
    "_build_month_conditions",
    "_build_day_conditions",
]

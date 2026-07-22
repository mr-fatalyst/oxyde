"""Unified type registry for the Oxyde ORM.

TYPE_REGISTRY maps Python types to their ORM-level descriptors:
    - category: lookup category for query filters
    - serialize: value → msgpack-safe value

Column type classification lives in `core/column_types.py`
(`compute_column_type`); this registry covers the remaining roles.
Uses exact type() lookup — no issubclass ordering issues (bool vs int).
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from datetime import date, datetime, time, timedelta
from decimal import Decimal
from enum import Enum
from typing import Any
from uuid import UUID


@dataclass(frozen=True, slots=True)
class TypeDescriptor:
    """Describes how a Python type maps to ORM operations."""

    category: str  # "string", "numeric", "datetime", "time", "bool", "generic"
    serialize: Callable[[Any], Any]  # value → msgpack-safe value


TYPE_REGISTRY: dict[type, TypeDescriptor] = {
    bool: TypeDescriptor("bool", lambda v: v),
    int: TypeDescriptor("numeric", lambda v: v),
    float: TypeDescriptor("numeric", lambda v: v),
    str: TypeDescriptor("string", lambda v: v),
    bytes: TypeDescriptor("generic", lambda v: v),
    bytearray: TypeDescriptor("generic", bytes),
    datetime: TypeDescriptor("datetime", lambda v: v.isoformat()),
    date: TypeDescriptor("datetime", lambda v: v.isoformat()),
    # Own category: comparisons apply, but a time-of-day has no calendar
    # part, so date-part lookups (year/month/day) must not be offered.
    time: TypeDescriptor("time", lambda v: v.isoformat()),
    timedelta: TypeDescriptor("generic", lambda v: int(v.total_seconds() * 1_000_000)),
    UUID: TypeDescriptor("generic", str),
    Decimal: TypeDescriptor("numeric", str),
    dict: TypeDescriptor("generic", lambda v: v),
}


def serialize_value(value: Any) -> Any:
    """Serialize a value for msgpack/IR using TYPE_REGISTRY.

    Handles lists and dicts recursively. Returns value unchanged if type is not
    in the registry (int, str, float, bool, None pass through as-is
    since msgpack handles them natively).
    """
    if isinstance(value, list):
        return [serialize_value(v) for v in value]
    if isinstance(value, dict):
        return {k: serialize_value(v) for k, v in value.items()}
    if isinstance(value, Enum):
        return value.value
    desc = TYPE_REGISTRY.get(type(value))
    if desc is not None:
        return desc.serialize(value)
    return value


__all__ = ["TypeDescriptor", "TYPE_REGISTRY", "serialize_value"]

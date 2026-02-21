"""Unified type registry for the Oxyde ORM.

TYPE_REGISTRY maps Python types to their ORM-level descriptors:
    - ir_name: type hint string for Rust IR decoding
    - category: lookup category for query filters
    - serialize: value → msgpack-safe value

All type-aware logic (IR mapping, lookup categories, value serialization)
delegates to this single registry. Uses exact type() lookup — no issubclass
ordering issues (bool vs int).
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from datetime import date, datetime, time, timedelta
from decimal import Decimal
from typing import Any
from uuid import UUID


@dataclass(frozen=True, slots=True)
class TypeDescriptor:
    """Describes how a Python type maps to ORM operations."""

    ir_name: str  # "uuid", "int", "datetime" — for Rust IR decoding
    category: str  # "string", "numeric", "datetime", "bool", "generic"
    serialize: Callable[[Any], Any]  # value → msgpack-safe value


TYPE_REGISTRY: dict[type, TypeDescriptor] = {
    bool: TypeDescriptor("bool", "bool", lambda v: v),
    int: TypeDescriptor("int", "numeric", lambda v: v),
    float: TypeDescriptor("float", "numeric", lambda v: v),
    str: TypeDescriptor("str", "string", lambda v: v),
    bytes: TypeDescriptor("bytes", "generic", lambda v: v),
    bytearray: TypeDescriptor("bytes", "generic", bytes),
    datetime: TypeDescriptor("datetime", "datetime", lambda v: v.isoformat()),
    date: TypeDescriptor("date", "datetime", lambda v: v.isoformat()),
    time: TypeDescriptor("time", "datetime", lambda v: v.isoformat()),
    timedelta: TypeDescriptor("timedelta", "generic", lambda v: v.total_seconds()),
    UUID: TypeDescriptor("uuid", "generic", str),
    Decimal: TypeDescriptor("decimal", "numeric", str),
    dict: TypeDescriptor("json", "generic", lambda v: v),
}


def serialize_value(value: Any) -> Any:
    """Serialize a value for msgpack/IR using TYPE_REGISTRY.

    Handles lists recursively. Returns value unchanged if type is not
    in the registry (int, str, float, bool, None pass through as-is
    since msgpack handles them natively).
    """
    if isinstance(value, list):
        return [serialize_value(v) for v in value]
    desc = TYPE_REGISTRY.get(type(value))
    if desc is not None:
        return desc.serialize(value)
    return value


__all__ = ["TypeDescriptor", "TYPE_REGISTRY", "serialize_value"]

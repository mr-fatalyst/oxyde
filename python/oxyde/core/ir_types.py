"""IR type mapping for Python types.

Maps Python types to IR type hint strings used by Rust for type-aware decoding.
This module is intentionally minimal to avoid circular imports.
"""

from __future__ import annotations

import datetime
import decimal
import uuid
from typing import Any, get_args, get_origin

# Mapping from Python types to IR type hints for Rust type-aware decoding
_PYTHON_TYPE_TO_IR: dict[type, str] = {
    int: "int",
    str: "str",
    float: "float",
    bool: "bool",
    bytes: "bytes",
    bytearray: "bytes",
    datetime.datetime: "datetime",
    datetime.date: "date",
    datetime.time: "time",
    datetime.timedelta: "timedelta",
    decimal.Decimal: "decimal",
    uuid.UUID: "uuid",
    dict: "json",
}


def get_ir_type(python_type: Any) -> str | None:
    """Convert Python type to IR type hint string.

    Returns None for unsupported types (fallback to dynamic decode in Rust).
    Handles Optional[T], dict[K, V], and other generic types.
    """
    origin = get_origin(python_type)

    # dict[K, V] -> "json"
    if origin is dict:
        return "json"

    # Union types (including Optional[T]) -> recurse into args
    if origin is not None:
        for arg in get_args(python_type):
            if arg is not type(None):
                result = get_ir_type(arg)
                if result:
                    return result
        return None

    # Simple types: int, str, dict, etc.
    return _PYTHON_TYPE_TO_IR.get(python_type)


__all__ = ["get_ir_type", "_PYTHON_TYPE_TO_IR"]

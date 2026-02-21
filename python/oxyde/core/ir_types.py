"""IR type mapping for Python types.

Maps Python types to IR type hint strings used by Rust for type-aware decoding.
Delegates to TYPE_REGISTRY for the leaf type lookup.
"""

from __future__ import annotations

from typing import Any, get_args, get_origin

from oxyde.core.types import TYPE_REGISTRY


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

    # Simple types: exact lookup in TYPE_REGISTRY
    desc = TYPE_REGISTRY.get(python_type)
    return desc.ir_name if desc is not None else None


__all__ = ["get_ir_type"]

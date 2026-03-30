"""Type introspection utilities for Model field parsing.

This module provides helper functions for extracting type information
from Python type hints. Used during model metadata parsing.

Functions:
    _unpack_annotated(hint) -> (base_type, metadata_tuple):
        Extract base type from Annotated[T, ...].
        Returns (hint, ()) if not Annotated.

        Annotated[int, Field(ge=0)]  →  (int, (Field(ge=0),))

    _unwrap_optional(hint) -> (inner_type, is_optional):
        Check if type is Optional[T] or T | None.
        Returns (T, True) if nullable, (hint, False) otherwise.

        int | None  →  (int, True)
        str         →  (str, False)

    _extract_constraints(field_info) -> dict[str, int]:
        Extract type constraints from FieldInfo or Annotated metadata.
        Used for VARCHAR(n), DECIMAL(m,d) type inference.

        Field(max_length=100)           →  {"max_length": 100}
        Field(max_digits=10, decimal_places=2)  →  {"max_digits": 10, "decimal_places": 2}
"""

from __future__ import annotations

from types import NoneType, UnionType
from typing import Annotated, Any, Union, get_args, get_origin

from pydantic.fields import FieldInfo


def _unpack_annotated(hint: Any) -> tuple[Any, tuple[Any, ...]]:
    """Extract base type and metadata from Annotated type hint."""
    if get_origin(hint) is Annotated:
        args = get_args(hint)
        if args:
            return args[0], tuple(args[1:])
    return hint, ()


def _unwrap_optional(hint: Any) -> tuple[Any, bool]:
    """Check if type is Optional/Union with None and extract base type."""
    origin = get_origin(hint)
    if origin in (Union, UnionType):
        args = []
        nullable = False
        for arg in get_args(hint):
            if arg is NoneType:
                nullable = True
            else:
                args.append(arg)
        if nullable and len(args) == 1:
            return args[0], True
    return hint, False


_CONSTRAINT_ATTRS = ("max_length", "max_digits", "decimal_places")


def _extract_constraints(field_info: FieldInfo) -> dict[str, int]:
    """Extract type constraints from FieldInfo or Annotated metadata.

    Checks both direct FieldInfo attributes and Annotated metadata tuple.
    Returns only constraints that are present and valid integers.
    """
    result: dict[str, int] = {}
    for attr in _CONSTRAINT_ATTRS:
        val = getattr(field_info, attr, None)
        if val is None:
            for meta in getattr(field_info, "metadata", ()):
                val = getattr(meta, attr, None)
                if val is not None:
                    break
        if val is not None:
            try:
                result[attr] = int(val)
            except (TypeError, ValueError):
                continue
    return result


__all__ = [
    "_unpack_annotated",
    "_unwrap_optional",
    "_extract_constraints",
]

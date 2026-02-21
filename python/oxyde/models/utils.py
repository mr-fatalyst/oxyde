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

    _extract_max_length(field_info) -> int | None:
        Extract max_length from FieldInfo or Annotated metadata.
        Used for VARCHAR(n) type inference.

        Field(max_length=100)  →  100
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


def _extract_max_length(field_info: FieldInfo) -> int | None:
    """Extract max_length constraint from FieldInfo or metadata."""
    candidates = []
    max_length_attr = getattr(field_info, "max_length", None)
    if max_length_attr is not None:
        candidates.append(max_length_attr)
    for meta in getattr(field_info, "metadata", ()):
        length = getattr(meta, "max_length", None)
        if length is not None:
            candidates.append(length)
    for value in candidates:
        try:
            return int(value)
        except (TypeError, ValueError):
            continue
    return None


__all__ = [
    "_unpack_annotated",
    "_unwrap_optional",
    "_extract_max_length",
]

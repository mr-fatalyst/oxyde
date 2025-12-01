"""Serialization utilities for INSERT/UPDATE operations.

This module handles conversion of OxydeModel instances to dict payloads
suitable for database operations. It filters out virtual fields and
handles Pydantic's model_dump() options.

Virtual Fields:
    Fields with db_reverse_fk or db_m2m are "virtual" - they represent
    relations loaded via JOINs but don't have actual database columns.
    These must be excluded from INSERT/UPDATE payloads.

    class Post(OxydeModel):
        id: int = Field(db_pk=True)
        author_id: int  # Real column
        comments: list[Comment] = Field(db_reverse_fk="post")  # Virtual

Functions:
    _get_virtual_fields(model_class) -> set[str]:
        Return names of virtual (relation) fields.

    _dump_insert_data(instance) -> dict:
        Serialize instance for INSERT. Uses model_dump(exclude_none=True).
        Excludes virtual fields.

    _dump_update_data(instance, fields) -> dict:
        Serialize specific fields for UPDATE. Includes None values.
        Excludes virtual fields.

    _derive_create_data(filters, defaults) -> dict:
        Merge filter kwargs with defaults for get_or_create().
        Only includes exact lookups (no __gte, __contains, etc.).

    _normalize_instance(model_class, payload) -> OxydeModel:
        Convert dict to model instance, or return instance as-is.
        Used by bulk_create() to accept mixed input.
"""

from __future__ import annotations

from collections.abc import Iterable
from typing import TYPE_CHECKING, Any

from oxyde.exceptions import ManagerError

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


def _get_virtual_fields(model_class: type[OxydeModel]) -> set[str]:
    """Get field names that are virtual (db_reverse_fk, db_m2m).

    These fields don't correspond to actual database columns and must be
    excluded from INSERT/UPDATE operations.
    """
    from oxyde.models.field import OxydeFieldInfo

    virtual: set[str] = set()
    for field_name, field_info in model_class.model_fields.items():
        if isinstance(field_info, OxydeFieldInfo):
            if getattr(field_info, "db_reverse_fk", None) or getattr(
                field_info, "db_m2m", False
            ):
                virtual.add(field_name)
    return virtual


def _dump_insert_data(instance: OxydeModel) -> dict[str, Any]:
    """Serialize model instance for INSERT operation.

    Excludes virtual relation fields (db_reverse_fk, db_m2m) that don't
    correspond to actual database columns.
    """
    # Get virtual field names to exclude
    virtual_fields = _get_virtual_fields(instance.__class__)
    data = instance.model_dump(mode="python", exclude_none=True, exclude=virtual_fields)
    return data


def _dump_update_data(instance: OxydeModel, fields: Iterable[str]) -> dict[str, Any]:
    """Serialize specific fields of model instance for UPDATE operation.

    Excludes virtual relation fields (db_reverse_fk, db_m2m) that don't
    correspond to actual database columns.
    """
    virtual_fields = _get_virtual_fields(instance.__class__)
    snapshot = instance.model_dump(mode="python", exclude_none=False)
    return {
        field: snapshot[field]
        for field in fields
        if field in snapshot and field not in virtual_fields
    }


def _derive_create_data(
    filters: dict[str, Any],
    defaults: dict[str, Any] | None,
) -> dict[str, Any]:
    """Derive data for create operation from filters and defaults."""
    data: dict[str, Any] = {}
    for key, value in filters.items():
        if "__" not in key:
            data[key] = value
    if defaults:
        data.update(defaults)
    return data


def _normalize_instance(
    model_class: type[OxydeModel],
    payload: Any,
) -> OxydeModel:
    """Normalize payload to model instance."""
    if isinstance(payload, model_class):
        return payload
    if isinstance(payload, dict):
        return model_class(**payload)
    raise ManagerError(
        f"Unsupported payload type for {model_class.__name__}: {type(payload).__name__}"
    )


__all__ = [
    "_get_virtual_fields",
    "_dump_insert_data",
    "_dump_update_data",
    "_derive_create_data",
    "_normalize_instance",
]

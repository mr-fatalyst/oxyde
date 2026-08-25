"""Serialization utilities for INSERT/UPDATE operations.

This module handles conversion of Model instances to dict payloads
suitable for database operations. It filters out virtual fields and
handles Pydantic's model_dump() options.

Virtual Fields:
    Fields with db_reverse_fk or db_m2m are "virtual" - they represent
    relations loaded via JOINs but don't have actual database columns.
    These must be excluded from INSERT/UPDATE payloads.

    class Post(Model):
        id: int = Field(db_pk=True)
        author_id: int  # Real column
        comments: list[Comment] = Field(db_reverse_fk="post")  # Virtual

Functions:
    _get_virtual_fields(model_class) -> set[str]:
        Return names of virtual (relation) fields.

    _dump_insert_data(instance) -> dict:
        Serialize instance for INSERT. Includes default/default_factory
        values; omits a column only when it is None and was not set
        explicitly. Excludes virtual fields.

    _dump_update_data(instance, fields) -> dict:
        Serialize specific fields for UPDATE. Includes None values.
        Excludes virtual fields.

    _derive_create_data(filters, defaults) -> dict:
        Merge filter kwargs with defaults for get_or_create().
        Only includes exact lookups (no __gte, __contains, etc.).

    _normalize_instance(model_class, payload) -> Model:
        Convert dict to model instance, or return instance as-is.
        Used by bulk_create() to accept mixed input.
"""

from __future__ import annotations

from collections.abc import Iterable
from typing import TYPE_CHECKING, Any

from oxyde.exceptions import ManagerError

if TYPE_CHECKING:
    from oxyde.models.base import Model


def _get_virtual_fields(model_class: type[Model]) -> set[str]:
    """Get field names that are virtual and don't correspond to database columns.

    Virtual fields include:
    - db_reverse_fk: reverse FK relations (e.g., posts: list[Post])
    - db_m2m: many-to-many relations (e.g., tags: list[Tag])
    - FK model fields: (e.g., author: Author) — real column is author_id
    """
    virtual: set[str] = set()
    for name, meta in model_class._db_meta.field_metadata.items():
        if meta.extra.get("reverse_fk") or meta.extra.get("m2m"):
            virtual.add(name)
        elif meta.foreign_key is not None:
            virtual.add(name)
    return virtual


def _dump_insert_data(instance: Model) -> dict[str, Any]:
    """Serialize model instance for INSERT operation.

    Excludes virtual relation fields (db_reverse_fk, db_m2m) and Pydantic
    computed fields that don't correspond to actual database columns.

    Values filled from ``default=`` / ``default_factory`` are included —
    dumping with ``exclude_unset`` dropped them, silently inserting NULL or
    the server default instead (issue #35). A column is omitted only when its
    value is None AND the field was not set explicitly: an untouched
    ``Field(default=None, db_pk=True)`` pk stays out of the INSERT so the
    database sequence fills it, while an explicit ``field=None`` still
    reaches the database as NULL (e.g. to override a server-side default).
    """
    virtual_fields = _get_virtual_fields(instance.__class__)
    computed_fields = set(instance.__class__.model_computed_fields.keys())
    data = instance.model_dump(mode="python", exclude=virtual_fields | computed_fields)
    explicitly_set = instance.model_fields_set
    return {
        name: value
        for name, value in data.items()
        if value is not None or name in explicitly_set
    }


def _dump_update_data(instance: Model, fields: Iterable[str]) -> dict[str, Any]:
    """Serialize specific fields of model instance for UPDATE operation.

    Excludes virtual relation fields (db_reverse_fk, db_m2m) and Pydantic
    computed fields that don't correspond to actual database columns.
    """
    virtual_fields = _get_virtual_fields(instance.__class__)
    computed_fields = set(instance.__class__.model_computed_fields.keys())
    excluded = virtual_fields | computed_fields
    snapshot = instance.model_dump(mode="python", exclude_none=False)
    return {
        field: snapshot[field]
        for field in fields
        if field in snapshot and field not in excluded
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
    model_class: type[Model],
    payload: Any,
) -> Model:
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

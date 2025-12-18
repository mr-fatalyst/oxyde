"""Dataclasses for storing parsed model and field metadata.

This module defines the internal data structures used to store metadata
extracted from OxydeModel class definitions. These are populated by
OxydeModel._parse_field_tags() during model initialization.

Hierarchy:
    ModelMeta (table-level)
        ├── table_name: str
        ├── schema: str (PostgreSQL schema)
        ├── indexes: list[Index]
        ├── constraints: list[Check]
        ├── field_metadata: dict[str, ColumnMeta]
        └── relations: dict[str, RelationInfo]

    ColumnMeta (field-level)
        ├── name: Field name in Python
        ├── db_column: Column name in database
        ├── python_type: Python type (str, int, datetime, etc.)
        ├── nullable: bool
        ├── primary_key: bool
        ├── index/unique: bool
        ├── foreign_key: ForeignKeyInfo | None
        └── db_default: SQL DEFAULT expression

    ForeignKeyInfo (FK constraint)
        ├── target: Target model key ("module.ClassName")
        ├── column_name: Column in this table ("author_id")
        ├── target_field: Target column ("id")
        └── on_delete/on_update: CASCADE, RESTRICT, etc.

    RelationInfo (virtual relation, not stored in DB)
        ├── kind: "one_to_many" | "many_to_many"
        ├── target: Target model name
        ├── remote_field: FK field on related model
        └── through: M2M junction table

Usage:
    # Access via model's _db_meta attribute
    User._db_meta.table_name  # "users"
    User._db_meta.field_metadata["email"].unique  # True
    User._db_meta.field_metadata["author"].foreign_key.target  # "myapp.Author"
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field as dataclass_field
from typing import Any

from pydantic.fields import PydanticUndefined

from oxyde.models.decorators import Check, Index


@dataclass(slots=True)
class ForeignKeyInfo:
    """Metadata describing a foreign key relationship."""

    target: (
        str  # Target model key (e.g., "__main__.User") or table name for primitive FK
    )
    column_name: str  # Column name in this table (e.g., "author_id")
    target_field: str  # Target field name in referenced table (e.g., "id", "uuid")
    nullable: bool
    on_delete: str | None = None
    on_update: str | None = None


@dataclass(slots=True)
class ColumnMeta:
    """Metadata captured for a single model field."""

    name: str
    db_column: str
    python_type: Any
    nullable: bool
    db_type: str | None = None
    index: bool = False
    index_name: str | None = None
    index_method: str | None = None  # btree, hash, gin, gist
    unique: bool = False
    primary_key: bool = False
    comment: str | None = None
    default: Any = PydanticUndefined
    default_factory: Any | None = None
    db_default: str | None = None  # Raw SQL DEFAULT expression
    max_length: int | None = None
    foreign_key: ForeignKeyInfo | None = None
    checks: list[str] = dataclass_field(default_factory=list)
    extra: dict[str, Any] = dataclass_field(default_factory=dict)


@dataclass(slots=True)
class RelationInfo:
    """Metadata describing a relationship between models."""

    name: str
    kind: str  # one_to_many, many_to_many
    target: str
    remote_field: str | None = None
    through: str | None = None
    back_populates: str | None = None


@dataclass(slots=True)
class ModelMeta:
    """Container for model metadata."""

    table_name: str | None = None
    schema: str | None = None
    comment: str | None = None
    indexes: list[Index] = dataclass_field(default_factory=list)
    unique_together: list[tuple[str, ...]] = dataclass_field(default_factory=list)
    constraints: list[Check] = dataclass_field(default_factory=list)
    field_metadata: dict[str, ColumnMeta] = dataclass_field(default_factory=dict)
    relations: dict[str, RelationInfo] = dataclass_field(default_factory=dict)
    extra: dict[str, Any] = dataclass_field(default_factory=dict)
    # Cached IR type hints for Rust decoding (computed in ensure_field_metadata)
    col_types: dict[str, str] | None = None


class RelationDescriptorBase:
    """Marker base class for relation descriptors."""

    _is_relation_descriptor = True


__all__ = [
    "ForeignKeyInfo",
    "ColumnMeta",
    "RelationInfo",
    "ModelMeta",
    "RelationDescriptorBase",
]

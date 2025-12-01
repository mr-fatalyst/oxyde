"""ORM model definitions and field configuration.

This module provides the core model system built on Pydantic v2:

Classes:
    OxydeModel: Base class for all database models. Provides:
        - Automatic table name generation from class name
        - QueryManager via Model.objects for CRUD operations
        - save(), delete(), refresh() instance methods
        - Lifecycle hooks: pre_save, post_save, pre_delete, post_delete

    Field: Enhanced field configuration function (wraps Pydantic Field):
        - db_pk: Mark as primary key (auto-increment if int)
        - db_index: Create single-column index
        - db_unique: Add UNIQUE constraint
        - db_default: Database-side DEFAULT value
        - db_column: Override column name
        - db_fk: Foreign key reference ("table.column")

    Index: Composite index decorator for Meta.indexes.
    Check: CHECK constraint decorator for Meta.constraints.

Model Registry:
    register_table(): Register model in global registry.
    unregister_table(): Remove model from registry.
    registered_tables(): Get dict of all registered models.
    iter_tables(): Iterate over registered models.
    clear_registry(): Remove all registered models.

Example:
    from oxyde import OxydeModel, Field, Index

    @Index("name", "email", unique=True)
    class User(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str = Field(db_index=True)
        email: str = Field(db_unique=True)

        class Meta:
            is_table = True
            table_name = "users"
"""

from .base import OxydeModel
from .decorators import (  # Keep Index for Meta.indexes, Check for Meta.constraints
    Check,
    Index,
)
from .field import Field, OxydeFieldInfo
from .registry import (
    clear_registry,
    iter_tables,
    register_table,
    registered_tables,
    unregister_table,
)

__all__ = [
    "OxydeModel",
    "Field",
    "OxydeFieldInfo",
    "Index",
    "Check",
    "register_table",
    "unregister_table",
    "registered_tables",
    "iter_tables",
    "clear_registry",
]

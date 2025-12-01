"""Table-level constraint definitions: Index and Check.

This module defines dataclasses for table-level constraints that cannot be
expressed at the field level. Use these in the model's Meta class.

When to use:
    Field-level (use Field()):
        - Single-column indexes: Field(db_index=True)
        - Single-column unique: Field(db_unique=True)
        - Primary key: Field(db_pk=True)

    Table-level (use this module):
        - Composite indexes: Index(("col1", "col2"))
        - Partial indexes: Index(("email",), where="deleted_at IS NULL")
        - Custom index methods: Index(("data",), method="gin")
        - CHECK constraints: Check("price > 0")

Classes:
    Index: Composite/partial index definition.
        fields: Column names as tuple
        method: btree | hash | gin | gist (default: btree)
        name: Custom index name (auto-generated if None)
        unique: Create UNIQUE index
        where: Partial index condition (SQL fragment)

    Check: CHECK constraint definition.
        expression: SQL boolean expression
        name: Constraint name (auto-generated if None)

Example:
    class Order(OxydeModel):
        user_id: int
        created_at: datetime
        total: Decimal
        status: str

        class Meta:
            is_table = True
            indexes = [
                Index(("user_id", "created_at")),
                Index(("status",), where="status != 'archived'"),
            ]
            constraints = [
                Check("total >= 0", name="positive_total"),
            ]
"""

from dataclasses import dataclass
from typing import Literal


@dataclass
class Index:
    """Index definition for Meta.indexes (table-level).

    Used for composite indexes and partial indexes.
    For single-field indexes, use Field(db_index=True) instead.

    Examples:
        >>> class User(OxydeModel):
        ...     class Meta:
        ...         indexes = [
        ...             Index(("city", "created_at"), method="btree"),
        ...             Index(("email",), unique=True, where="deleted_at IS NULL"),
        ...         ]
    """

    fields: tuple[str, ...]
    method: Literal["btree", "hash", "gin", "gist"] | None = None
    name: str | None = None
    unique: bool = False
    where: str | None = None  # Partial index condition

    def __init__(
        self,
        fields: tuple[str, ...] | list[str],
        method: Literal["btree", "hash", "gin", "gist"] | None = None,
        name: str | None = None,
        unique: bool = False,
        where: str | None = None,
    ):
        if not fields:
            raise ValueError("Index requires at least one field")
        self.fields = tuple(fields) if isinstance(fields, list) else fields
        self.method = method
        self.name = name
        self.unique = unique
        self.where = where


@dataclass
class Check:
    """Check constraint definition for Meta.constraints (table-level).

    Examples:
        >>> class Event(OxydeModel):
        ...     start_date: datetime
        ...     end_date: datetime
        ...     class Meta:
        ...         constraints = [
        ...             Check("start_date < end_date", name="valid_dates"),
        ...         ]
    """

    expression: str
    name: str | None = None


__all__ = ["Index", "Check"]

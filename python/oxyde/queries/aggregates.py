"""SQL aggregate and scalar functions for SELECT queries.

This module provides Python wrappers for SQL aggregate functions
used with annotate() and direct aggregate queries.

Aggregate Functions:
    Count(field="*", distinct=False) → COUNT(*) or COUNT(DISTINCT field)
    Sum(field)                       → SUM(field)
    Avg(field)                       → AVG(field)
    Max(field)                       → MAX(field)
    Min(field)                       → MIN(field)

Scalar Functions:
    Concat(*fields, separator="")    → Concatenate strings
    Coalesce(*fields)                → First non-NULL value

Raw SQL:
    RawSQL(sql)                      → Inject raw SQL expression

Usage with annotate():
    # Add computed columns to query results
    query = User.objects.annotate(
        post_count=Count("id"),
        total_score=Sum("score"),
    )

Usage with group_by():
    query = Order.objects.annotate(
        total=Sum("amount"),
    ).group_by("customer_id")

Direct aggregates (execute immediately):
    total = await User.objects.filter(active=True).count()
    avg_age = await User.objects.avg("age")
    max_score = await User.objects.max("score")

IR Format:
    Count("id").to_ir() returns:
    {"func": "count", "field": "id"}

    Count("id", distinct=True).to_ir() returns:
    {"func": "count", "field": "id", "distinct": True}
"""

from __future__ import annotations

from typing import Any


class Aggregate:
    """Base class for aggregate functions."""

    def __init__(self, field: str, *, distinct: bool = False):
        self.field = field
        self.distinct = distinct
        self.func_name = self.__class__.__name__.lower()

    def to_ir(self) -> dict[str, Any]:
        """Convert to IR representation."""
        ir: dict[str, Any] = {
            "func": self.func_name,
            "field": self.field,
        }
        if self.distinct:
            ir["distinct"] = True
        return ir


class Count(Aggregate):
    """COUNT aggregate function."""

    def __init__(self, field: str = "*", *, distinct: bool = False):
        super().__init__(field, distinct=distinct)


class Sum(Aggregate):
    """SUM aggregate function."""

    pass


class Avg(Aggregate):
    """AVG aggregate function."""

    pass


class Max(Aggregate):
    """MAX aggregate function."""

    pass


class Min(Aggregate):
    """MIN aggregate function."""

    pass


class Concat(Aggregate):
    """String concatenation."""

    def __init__(self, *fields: str, separator: str = ""):
        self.fields = fields
        self.separator = separator
        self.func_name = "concat"

    def to_ir(self) -> dict[str, Any]:
        return {
            "func": "concat",
            "fields": list(self.fields),
            "separator": self.separator,
        }


class Coalesce(Aggregate):
    """COALESCE function - returns first non-NULL value."""

    def __init__(self, *fields: str):
        self.fields = fields
        self.func_name = "coalesce"

    def to_ir(self) -> dict[str, Any]:
        return {
            "func": "coalesce",
            "fields": list(self.fields),
        }


class RawSQL:
    """Raw SQL expression."""

    def __init__(self, sql: str):
        self.sql = sql

    def to_ir(self) -> dict[str, Any]:
        return {
            "type": "raw",
            "sql": self.sql,
        }


__all__ = [
    "Aggregate",
    "Count",
    "Sum",
    "Avg",
    "Max",
    "Min",
    "Concat",
    "Coalesce",
    "RawSQL",
]

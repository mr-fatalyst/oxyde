"""Condition class representing a single WHERE clause comparison.

Condition is the leaf node of the filter tree. It represents a single
comparison like "age >= 18" or "name LIKE '%alice%'".

Attributes:
    field: Python field name (e.g., "age")
    operator: SQL operator ("=", ">", "LIKE", "IN", "IS NULL", etc.)
    value: Comparison value (can be None for IS NULL)
    column: Database column name (may differ from field if db_column set)

Operators:
    "="        → Equality
    ">", ">="  → Greater than
    "<", "<="  → Less than
    "LIKE"     → Pattern match (case-sensitive)
    "ILIKE"    → Pattern match (case-insensitive, PostgreSQL)
    "IN"       → Value in list
    "BETWEEN"  → Range (value is [low, high])
    "IS NULL"  → NULL check
    "IS NOT NULL" → NOT NULL check

Creation:
    Conditions are typically created via Field comparison operators:
        User.name == "Alice"  → Condition("name", "=", "Alice")
        User.age >= 18        → Condition("age", ">=", 18)

    Or via lookups module:
        _build_lookup_conditions(User, "name", "contains", "alice", meta)
        → Condition("name", "LIKE", "%alice%")

IR Format:
    to_ir() returns dict for Rust codec:
    {
        "type": "condition",
        "field": "age",
        "op": ">=",
        "value": 18,
        "column": "age"
    }
"""

from __future__ import annotations

from typing import Any

from oxyde.core import ir


class Condition:
    """Represents a filter condition."""

    def __init__(
        self, field: str, operator: str, value: Any, *, column: str | None = None
    ):
        self.field = field
        self.operator = operator
        self.value = value
        self.column = column or field

    def to_ir(self) -> dict[str, Any]:
        """Convert to IR format expected by the Rust codec."""
        return ir.filter_condition(
            field=self.field,
            operator=self.operator,
            value=self.value,
            column=self.column,
        )


__all__ = ["Condition"]

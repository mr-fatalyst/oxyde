"""Query building system for Oxyde ORM.

This module provides Django-style query building via Model.objects interface.
Queries are converted to IR (Intermediate Representation) and sent to the
Rust core for SQL generation.

Query Building:
    Use Model.objects for all queries:
        - filter(), exclude(): Add WHERE conditions
        - order_by(), limit(), offset(): Pagination
        - join(), prefetch(): Related objects
        - values(), values_list(): Dict/tuple results
        - annotate(), group_by(), having(): Aggregation

Execution Methods:
    - all(): Get all matching objects
    - first(), last(): Get single object
    - get(): Get exactly one object (raises if not found)
    - get_or_none(): Get one or None
    - count(), exists(): Aggregate queries
    - create(), bulk_create(): Insert objects
    - update(), delete(): Modify/remove objects

Query Expressions:
    F: Database-side field reference (F("price") * F("quantity")).
    Q: Boolean expression combinator (Q(a=1) | Q(b=2), ~Q(deleted=True)).

Aggregates:
    Count, Sum, Avg, Max, Min: SQL aggregate functions.
    Concat: String concatenation.
    Coalesce: NULL handling (COALESCE(a, b, c)).
    RawSQL: Escape hatch for raw SQL expressions.

Example:
    # Get active adult users
    users = await User.objects.filter(age__gte=18, status="active").all()

    # Complex query
    users = await (
        User.objects
        .filter(Q(role="admin") | Q(is_superuser=True))
        .exclude(deleted=True)
        .order_by("-created_at")
        .limit(10)
        .all()
    )
"""

from .aggregates import Aggregate, Avg, Coalesce, Concat, Count, Max, Min, RawSQL, Sum
from .base import SupportsExecute
from .expressions import F
from .manager import QueryManager
from .q import Q
from .raw import execute_raw
from .select import Query

__all__ = [
    "Query",
    "F",
    "Q",
    "SupportsExecute",
    "QueryManager",
    "Aggregate",
    "Count",
    "Sum",
    "Avg",
    "Max",
    "Min",
    "Concat",
    "Coalesce",
    "RawSQL",
    "execute_raw",
]

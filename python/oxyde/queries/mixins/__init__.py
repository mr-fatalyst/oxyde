"""Query mixins providing modular functionality for Query class.

The Query class inherits from all these mixins, each providing a specific
category of query operations. This keeps the code organized and testable.

Mixins:
    FilteringMixin:
        filter(), exclude() - WHERE clause building

    PaginationMixin:
        select() - choose specific columns
        limit(), offset() - result pagination
        order_by() - ORDER BY clause
        distinct() - DISTINCT modifier
        values(), values_list() - return dicts/tuples instead of models
        __getitem__ - slice syntax for limit/offset

    JoiningMixin:
        join() - eager loading with SQL JOINs
        prefetch() - prefetch related objects (separate queries)

    AggregationMixin:
        annotate() - add computed columns (COUNT, SUM, etc.)
        group_by() - GROUP BY clause
        having() - HAVING clause
        count(), sum(), avg(), max(), min() - terminal aggregates

    ExecutionMixin:
        fetch_all(), fetch_one() - raw execution returning dicts
        fetch_rows() - raw execution returning list of tuples
        fetch_models() - execution returning model instances
        fetch_msgpack() - execution returning raw MessagePack bytes
        all(), first(), last(), exists() - convenience methods

    MutationMixin:
        create(), bulk_create() - INSERT operations
        update(), bulk_update() - UPDATE operations
        delete() - DELETE from SELECT query
        increment() - atomic increment via F()

    DebugMixin:
        sql() - get generated SQL string
        query - property to inspect IR dict
        explain() - get query execution plan
        union(), union_all() - combine queries

Design:
    Each mixin expects certain attributes on self (from Query.__init__).
    Mixins use _clone() to maintain immutability.
"""

from oxyde.queries.mixins.aggregation import AggregationMixin
from oxyde.queries.mixins.debug import DebugMixin
from oxyde.queries.mixins.execution import ExecutionMixin
from oxyde.queries.mixins.filtering import FilteringMixin
from oxyde.queries.mixins.joining import JoiningMixin
from oxyde.queries.mixins.mutation import MutationMixin
from oxyde.queries.mixins.pagination import PaginationMixin

__all__ = [
    "FilteringMixin",
    "PaginationMixin",
    "JoiningMixin",
    "AggregationMixin",
    "ExecutionMixin",
    "MutationMixin",
    "DebugMixin",
]

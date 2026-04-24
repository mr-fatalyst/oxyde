"""SELECT query builder with Django-style fluent interface.

This module provides the Query class - the primary way to build SELECT queries
in Oxyde. Query objects are immutable: each method returns a new Query instance.

Architecture:
    Query inherits from multiple mixins that provide different capabilities:
        FilteringMixin: filter(), exclude()
        PaginationMixin: limit(), offset(), order_by(), distinct(), values()
        JoiningMixin: join(), prefetch()
        AggregationMixin: annotate(), group_by(), having(), count(), sum()
        ExecutionMixin: fetch_all(), fetch_models(), exists(), all()
        MutationMixin: update(), delete(), increment()
        DebugMixin: sql(), explain(), union()

Immutability:
    All methods return new Query instances via _clone(). The original is unchanged.

    base = User.objects.filter(active=True)
    admins = base.filter(role="admin")  # base is unchanged
    users = base.filter(role="user")    # base is unchanged

State Attributes:
    _filter_tree: FilterNode tree for WHERE clause
    _limit_value: LIMIT value
    _offset_value: OFFSET value
    _order_by_fields: List of (field, direction) tuples
    _selected_fields: Explicit field list or None for all
    _distinct: DISTINCT flag
    _join_specs: JOIN descriptors
    _annotations: Aggregate aliases {alias: Aggregate}
    _group_by_fields: GROUP BY columns
    _lock_type: "update" | "share" | None for row locking

Execution:
    to_ir() converts Query state to dict (Intermediate Representation).
    IR is serialized to MessagePack and sent to Rust for SQL generation.

    The IR includes col_types (dict mapping column names to IR type hints)
    which enables type-aware decoding in Rust without expensive type_info() calls.

Example:
    query = (
        User.objects
        .filter(status="active")
        .exclude(role="bot")
        .order_by("-created_at")
        .limit(10)
    )
    ir = query.to_ir()
    # {"table": "user", "columns": [...], "col_types": {"id": "int", ...}, ...}
"""

from __future__ import annotations

from collections.abc import Coroutine
from typing import TYPE_CHECKING, Any, Generic, TypeVar

from typing_extensions import Self

from oxyde.core import ir
from oxyde.models.utils import _unwrap_optional
from oxyde.queries.base import _model_key
from oxyde.queries.joins import _JoinDescriptor
from oxyde.queries.mixins import (
    AggregationMixin,
    DebugMixin,
    ExecutionMixin,
    FilteringMixin,
    JoiningMixin,
    MutationMixin,
    PaginationMixin,
)

if TYPE_CHECKING:
    from oxyde.models.base import Model


TModel = TypeVar("TModel", bound="Model")


class Query(
    FilteringMixin,
    PaginationMixin,
    JoiningMixin,
    AggregationMixin,
    ExecutionMixin,
    MutationMixin,
    DebugMixin,
    Generic[TModel],
):
    """
    Query builder for SELECT operations.

    Combines all query capabilities through mixins:
    - FilteringMixin: filter, exclude
    - PaginationMixin: limit, offset, order_by, distinct, values, values_list
    - JoiningMixin: join, prefetch
    - AggregationMixin: annotate, group_by, having, count, sum, avg, max, min
    - ExecutionMixin: fetch_all, fetch_one, fetch_models, exists, all
    - MutationMixin: update, delete, increment
    - DebugMixin: sql, query, explain, union, union_all
    """

    def __init__(self, model_class: type[Model]):
        self.model_class = model_class
        # Filtering state
        self._filter_tree: ir.FilterNode | None = None
        # Pagination state
        self._limit_value: int | None = None
        self._offset_value: int | None = None
        self._order_by_fields: list[tuple[str, str]] = []
        self._selected_fields: list[str] | None = None
        self._distinct: bool = False
        self._result_mode: str | None = None
        self._values_flat: bool = False
        # Joining state
        self._join_specs: list[_JoinDescriptor] = []
        self._prefetch_paths: list[str] = []
        # Aggregation state
        self._annotations: dict[str, Any] = {}
        self._group_by_fields: list[str] = []
        self._having: ir.FilterNode | None = None
        # Union state
        self._union_query: Query | None = None
        self._union_all: bool = False
        # Locking state (FOR UPDATE / FOR SHARE)
        self._lock_type: str | None = None
        # Shortcut query flags (count / exists)
        self._count: bool = False
        self._exists: bool = False

    def _clone(self) -> Self:
        """Create a copy of this query."""
        clone = self.__class__(self.model_class)
        # Filtering
        clone._filter_tree = self._filter_tree
        # Pagination
        clone._limit_value = self._limit_value
        clone._offset_value = self._offset_value
        clone._order_by_fields = list(self._order_by_fields)
        clone._selected_fields = (
            None if self._selected_fields is None else list(self._selected_fields)
        )
        clone._distinct = self._distinct
        clone._result_mode = self._result_mode
        clone._values_flat = self._values_flat
        # Joining
        clone._join_specs = list(self._join_specs)
        clone._prefetch_paths = list(self._prefetch_paths)
        # Aggregation
        clone._annotations = dict(self._annotations)
        clone._group_by_fields = list(self._group_by_fields)
        clone._having = self._having
        # Union
        clone._union_query = self._union_query
        clone._union_all = self._union_all
        # Locking
        clone._lock_type = self._lock_type
        # Shortcut flags
        clone._count = self._count
        clone._exists = self._exists
        return clone

    def select(self, *fields: str) -> Self:
        """Specify fields to select."""
        if not fields:
            raise ValueError("select() requires at least one column name")
        clone = self._clone()
        clone._selected_fields = list(fields)
        return clone

    def for_update(self) -> Self:
        """Add FOR UPDATE lock to query.

        Locks selected rows for update, preventing other transactions
        from modifying them until the current transaction commits.

        Note: On SQLite this is a no-op (SQLite uses database-level locking).

        Returns:
            Query with FOR UPDATE lock

        Examples:
            async with atomic():
                user = await User.objects.filter(id=1).for_update().first()
                user.balance -= 100
                await user.save()
        """
        clone = self._clone()
        clone._lock_type = "update"
        return clone

    def for_share(self) -> Self:
        """Add FOR SHARE lock to query.

        Locks selected rows for reading, preventing other transactions
        from modifying them (but allowing reads) until the current
        transaction commits.

        Note: On SQLite this is a no-op (SQLite uses database-level locking).

        Returns:
            Query with FOR SHARE lock

        Examples:
            async with atomic():
                users = await User.objects.filter(status="active").for_share().all()
                # Other transactions can read but not modify these rows
        """
        clone = self._clone()
        clone._lock_type = "share"
        return clone

    def to_ir(self) -> dict[str, Any]:
        """Convert query to IR format for Rust execution."""
        from oxyde.models.base import Model

        table_name = self.model_class.get_table_name()

        # Get fields to select (exclude virtual relation fields and FK model fields)
        if self._selected_fields is None:
            fields = []
            for field_name, field_info in self.model_class.model_fields.items():
                # Skip virtual relation fields (db_reverse_fk, db_m2m)
                if getattr(field_info, "db_reverse_fk", None) or getattr(
                    field_info, "db_m2m", False
                ):
                    continue
                # Skip FK model fields (user: User) - we select user_id instead
                annotation = field_info.annotation
                if annotation is not None:
                    inner_type, _ = _unwrap_optional(annotation)
                    if isinstance(inner_type, type) and issubclass(inner_type, Model):
                        continue
                fields.append(field_name)
        else:
            if not self._selected_fields:
                raise ValueError("SELECT query must include at least one column")
            fields = self._selected_fields

        # Convert field names to db_columns for Rust (Rust operates on columns only)
        db_columns = [self._column_for_field(f) for f in fields]
        order_by = [
            (field if field == "?" else self._column_for_field(field), direction)
            for field, direction in self._order_by_fields
        ]

        # Build filter_tree
        final_filter_tree = self._build_filter_tree()

        # Convert annotations to aggregates IR format (supports multiple aggregates)
        aggregates_ir = None
        if self._annotations:
            aggregates_ir = []
            for alias, agg_obj in self._annotations.items():
                if not hasattr(agg_obj, "to_ir"):
                    raise TypeError(f"annotate() received non-expression: {agg_obj!r}")
                agg_spec = agg_obj.to_ir()
                func_name = agg_spec.get("func", "").lower()
                agg_ir: dict[str, Any] = {
                    "op": func_name,
                    "field": agg_spec.get("field"),
                    "alias": alias,
                }
                if agg_spec.get("distinct"):
                    agg_ir["distinct"] = True
                aggregates_ir.append(agg_ir)

        # Convert group_by fields to column names
        group_by_columns = None
        if self._group_by_fields:
            group_by_columns = [
                self._column_for_field(field) for field in self._group_by_fields
            ]

        # Use cached col_types from model metadata (computed at finalization)
        col_types = self.model_class._db_meta.col_types

        # Pass pk_column only for JOIN queries (needed for deduplication)
        pk_column = None
        if self._join_specs:
            pk_column = self.model_class._db_meta.pk_column

        # Build union IR recursively
        union_ir = None
        if self._union_query is not None:
            union_ir = self._union_query.to_ir()

        return ir.build_select_ir(
            table=table_name,
            columns=db_columns,
            col_types=col_types,
            model=_model_key(self.model_class),
            column_mappings=None,
            filter_tree=final_filter_tree,
            distinct=self._distinct or None,
            limit=self._limit_value,
            offset=self._offset_value,
            order_by=order_by or None,
            joins=self._join_specs_to_ir() or None,
            group_by=group_by_columns,
            having=self._having,
            aggregates=aggregates_ir,
            lock=self._lock_type,
            pk_column=pk_column,
            union_query=union_ir,
            union_all=self._union_all or None,
            count=self._count or None,
            exists=self._exists or None,
        )

    # Narrow inherited mixin return types from Model → TModel for type checkers.
    # These stubs are only visible to type checkers; the mixin implementations
    # run at runtime (they are already correct since model_class carries the type).
    if TYPE_CHECKING:
        from collections.abc import Iterable

        from oxyde.queries.base import SupportsExecute

        async def fetch_models(  # type: ignore
            self, client: SupportsExecute
        ) -> list[TModel]: ...

        def all(  # type: ignore
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> Coroutine[Any, Any, list[TModel]]: ...

        async def get(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> TModel: ...

        async def get_or_none(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> TModel | None: ...

        async def first(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> TModel | None: ...

        async def last(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> TModel | None: ...

        async def create(
            self,
            *,
            instance: Model | None = None,
            using: str | None = None,
            client: SupportsExecute | None = None,
            _skip_hooks: bool = False,
            **data: Any,
        ) -> TModel: ...

        async def bulk_create(  # type: ignore
            self,
            objects: Iterable[Any],
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
            batch_size: int | None = None,
        ) -> list[TModel]: ...


__all__ = ["Query"]

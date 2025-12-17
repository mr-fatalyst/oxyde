"""Aggregation mixin for query building."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import msgpack

from oxyde.core import ir
from oxyde.queries.base import SupportsExecute, TQuery, _resolve_execution_client

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


class AggregationMixin:
    """Mixin providing aggregation capabilities."""

    # These attributes are defined in the base Query class
    model_class: type[OxydeModel]
    _annotations: dict[str, Any]
    _group_by_fields: list[str]
    _having: ir.FilterNode | None
    _limit_value: int | None
    _offset_value: int | None
    _order_by_fields: list[tuple[str, str]]

    def _clone(self: TQuery) -> TQuery:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def to_ir(self) -> dict[str, Any]:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def _build_filter_tree(self) -> ir.FilterNode | None:
        """Must be implemented by FilteringMixin."""
        raise NotImplementedError

    def _join_specs_to_ir(self) -> list[dict[str, Any]]:
        """Must be implemented by JoiningMixin."""
        raise NotImplementedError

    def annotate(self: TQuery, **annotations) -> TQuery:
        """
        Add computed fields using aggregate functions.

        Args:
            **annotations: Named aggregate expressions

        Examples:
            from oxyde.queries.aggregates import Count, Avg
            User.objects.annotate(post_count=Count("posts"), avg_age=Avg("age"))
        """
        clone = self._clone()
        clone._annotations.update(annotations)
        return clone

    def group_by(self: TQuery, *fields: str) -> TQuery:
        """
        Add GROUP BY clause.

        Args:
            *fields: Field names to group by

        Examples:
            User.objects.group_by("status", "country")
        """
        clone = self._clone()
        clone._group_by_fields.extend(fields)
        return clone

    def having(self: TQuery, *q_exprs, **kwargs) -> TQuery:
        """
        Add HAVING clause for filtering grouped results.

        Args:
            *q_exprs: Q expressions
            **kwargs: Field lookups

        Examples:
            User.objects.group_by("status").having(count__gte=10)
        """
        from oxyde.queries.q import Q

        clone = self._clone()
        conditions_to_add: list[ir.FilterNode] = []

        for q_expr in q_exprs:
            if isinstance(q_expr, Q):
                node = q_expr.to_filter_node(self.model_class)
                if node:
                    conditions_to_add.append(node)

        if kwargs:
            q_from_kwargs = Q(**kwargs)
            node = q_from_kwargs.to_filter_node(self.model_class)
            if node:
                conditions_to_add.append(node)

        if conditions_to_add:
            if clone._having:
                conditions_to_add.insert(0, clone._having)

            if len(conditions_to_add) == 1:
                clone._having = conditions_to_add[0]
            else:
                clone._having = ir.filter_and(*conditions_to_add)

        return clone

    async def _aggregate(
        self,
        agg_class,
        field: str,
        result_key: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Execute an aggregate query and return the result."""
        exec_client = await _resolve_execution_client(using, client)

        # Build aggregate query (without limit/offset/order)
        agg_query = self._clone()
        agg_query._limit_value = None
        agg_query._offset_value = None
        agg_query._order_by_fields = []
        agg_query = agg_query.annotate(**{result_key: agg_class(field)})

        # Execute and extract value
        query_ir = agg_query.to_ir()
        result_bytes = await exec_client.execute(query_ir)
        result = msgpack.unpackb(result_bytes, raw=False)

        # Handle columnar format: (columns, rows)
        if isinstance(result, (list, tuple)) and len(result) == 2:
            first, second = result
            if isinstance(first, list) and all(isinstance(c, str) for c in first):
                # Columnar format
                columns = first
                rows = second
                if rows:
                    row_dict = dict(zip(columns, rows[0]))
                    return row_dict.get(result_key)
                return None

        if isinstance(result, list) and len(result) > 0:
            row = result[0]
            if isinstance(row, dict):
                return row.get(result_key)
            else:
                return getattr(row, result_key, None)
        return None

    async def count(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> int:
        """
        Count the number of records matching the query.

        Returns:
            int: Number of records

        Examples:
            count = await User.objects.filter(is_active=True).count()
        """
        exec_client = await _resolve_execution_client(using, client)

        # Build minimal count IR directly
        query_ir = ir.build_select_ir(
            table=self.model_class.get_table_name(),
            filter_tree=self._build_filter_tree(),
            joins=self._join_specs_to_ir() or None,
            count=True,
        )

        result_bytes = await exec_client.execute(query_ir)
        result = msgpack.unpackb(result_bytes, raw=False)

        # Handle columnar format: (columns, rows)
        if isinstance(result, (list, tuple)) and len(result) == 2:
            first, second = result
            if isinstance(first, list) and all(isinstance(c, str) for c in first):
                # Columnar format
                columns = first
                rows = second
                if rows:
                    row_dict = dict(zip(columns, rows[0]))
                    return row_dict.get("_count", 0) or 0
                return 0

        # Result is [{"_count": N}]
        if isinstance(result, list) and len(result) > 0:
            row = result[0]
            if isinstance(row, dict):
                return row.get("_count", 0) or 0
        return 0

    async def sum(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Calculate sum of field values."""
        from oxyde.queries.aggregates import Sum

        return await self._aggregate(Sum, field, "_sum", using=using, client=client)

    async def avg(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Calculate average of field values."""
        from oxyde.queries.aggregates import Avg

        return await self._aggregate(Avg, field, "_avg", using=using, client=client)

    async def max(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Get maximum field value."""
        from oxyde.queries.aggregates import Max

        return await self._aggregate(Max, field, "_max", using=using, client=client)

    async def min(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Get minimum field value."""
        from oxyde.queries.aggregates import Min

        return await self._aggregate(Min, field, "_min", using=using, client=client)

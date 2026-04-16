"""Aggregation mixin for query building."""

from __future__ import annotations

from decimal import Decimal
from typing import TYPE_CHECKING, Any

from typing_extensions import Self

from oxyde._msgpack import msgpack
from oxyde.core import ir
from oxyde.models.lookups import ALL_LOOKUPS
from oxyde.queries.aggregates import Avg, Max, Min, Sum
from oxyde.queries.base import SupportsExecute, _resolve_execution_client
from oxyde.queries.q import Q

if TYPE_CHECKING:
    from oxyde.models.base import Model

# Lookup → SQL operator mapping for annotation fields in HAVING
_HAVING_LOOKUP_OPS: dict[str, str] = {
    "exact": "=",
    "gt": ">",
    "gte": ">=",
    "lt": "<",
    "lte": "<=",
}


class AggregationMixin:
    """Mixin providing aggregation capabilities."""

    # These attributes are defined in the base Query class
    model_class: type[Model]
    _annotations: dict[str, Any]
    _group_by_fields: list[str]
    _having: ir.FilterNode | None
    _limit_value: int | None
    _offset_value: int | None
    _order_by_fields: list[tuple[str, str]]
    _count: bool

    def _clone(self) -> Self:
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

    def annotate(self, **annotations: Any) -> Self:
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

    def group_by(self, *fields: str) -> Self:
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

    def having(self, *q_exprs: Q | Any, **kwargs: Any) -> Self:
        """
        Add HAVING clause for filtering grouped results.

        Supports both annotation aliases and model fields:
            .having(total__gt=100)   # 'total' from annotate(total=Sum(...))
            .having(Q(total__gt=100) | Q(total__lt=10))

        Args:
            *q_exprs: Q expressions
            **kwargs: Field lookups

        Examples:
            Post.objects.annotate(total=Sum("views")).group_by("author_id").having(total__gt=100)
        """
        clone = self._clone()
        conditions_to_add: list[ir.FilterNode] = []

        for q_expr in q_exprs:
            if isinstance(q_expr, Q):
                node = q_expr.to_filter_node(self.model_class)
                if node:
                    conditions_to_add.append(node)

        if kwargs:
            annotation_kwargs = {}
            model_kwargs = {}
            for key, value in kwargs.items():
                field_name, _ = _split_having_key(key)
                if field_name in self._annotations:
                    annotation_kwargs[key] = value
                else:
                    model_kwargs[key] = value

            for key, value in annotation_kwargs.items():
                field_name, lookup = _split_having_key(key)
                operator = _HAVING_LOOKUP_OPS.get(lookup)
                if operator is None:
                    raise ValueError(
                        f"Unsupported lookup '{lookup}' for annotation "
                        f"'{field_name}' in having(). "
                        f"Supported: {', '.join(sorted(_HAVING_LOOKUP_OPS))}"
                    )
                conditions_to_add.append(
                    ir.filter_condition(field_name, operator, value)
                )

            if model_kwargs:
                node = Q(**model_kwargs).to_filter_node(self.model_class)
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
        agg_class: type,
        field: str,
        result_key: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> Any:
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
                    value = row_dict.get(result_key)
                    return self._coerce_aggregate(value, field, agg_class)
                return None

        if isinstance(result, list) and len(result) > 0:
            row = result[0]
            if isinstance(row, dict):
                return row.get(result_key)
            else:
                return getattr(row, result_key, None)
        return None

    def _coerce_aggregate(self, value: Any, field: str, agg_class: type) -> Any:
        """Coerce aggregate result to the field's Python type.

        PG returns NUMERIC for SUM/AVG even on integer fields.
        Rust encodes NUMERIC as string. Coerce back using field metadata.
        """
        if value is None or not isinstance(value, str):
            return value

        meta = self.model_class._db_meta.field_metadata.get(field)
        if meta is None:
            return value

        python_type = meta.python_type

        # AVG always returns float/Decimal, even for int fields
        if agg_class is Avg:
            if python_type is Decimal:
                return Decimal(value)
            return float(value)

        if python_type is int:
            return int(value)
        if python_type is float:
            return float(value)
        if python_type is Decimal:
            return Decimal(value)

        return value

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

        # Clone query with count flag — goes through to_ir() so col_types
        # and all other IR fields are included automatically.
        clone = self._clone()
        clone._count = True
        clone._limit_value = None
        clone._offset_value = None
        clone._order_by_fields = []
        query_ir = clone.to_ir()

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
    ) -> Any:
        """Calculate sum of field values."""
        return await self._aggregate(Sum, field, "_sum", using=using, client=client)

    async def avg(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> Any:
        """Calculate average of field values."""
        return await self._aggregate(Avg, field, "_avg", using=using, client=client)

    async def max(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> Any:
        """Get maximum field value."""
        return await self._aggregate(Max, field, "_max", using=using, client=client)

    async def min(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> Any:
        """Get minimum field value."""
        return await self._aggregate(Min, field, "_min", using=using, client=client)


def _split_having_key(key: str) -> tuple[str, str]:
    """Split a having kwarg key into (field_name, lookup).

    Examples:
        "total__gt" -> ("total", "gt")
        "total"     -> ("total", "exact")
    """
    if "__" not in key:
        return key, "exact"
    parts = key.rsplit("__", 1)
    if parts[1] in ALL_LOOKUPS:
        return parts[0], parts[1]
    return key, "exact"

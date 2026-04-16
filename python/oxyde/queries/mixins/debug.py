"""Debug mixin for query building."""

from __future__ import annotations

from collections.abc import Coroutine
from typing import TYPE_CHECKING, Any

from typing_extensions import Self

from oxyde._msgpack import msgpack
from oxyde.core.wrapper import explain_query, render_sql_debug
from oxyde.db.pool import _msgpack_encoder
from oxyde.queries.base import SupportsExecute, _resolve_pool_name

if TYPE_CHECKING:
    from oxyde.models.base import Model


class DebugMixin:
    """Mixin providing debugging and introspection capabilities."""

    # These attributes are defined in the base Query class
    model_class: type[Model]
    _union_query: DebugMixin | None
    _union_all: bool

    def _clone(self) -> Self:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def to_ir(self) -> dict[str, Any]:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def sql(
        self,
        *,
        dialect: str = "postgres",
        with_types: bool = False,
    ) -> tuple[str, list[Any]]:
        """
        Return SQL representation and parameters for debugging.

        This calls Rust's render_sql_debug() to generate real SQL using the same
        logic that executes queries, ensuring consistency. Works without database
        connection.

        Args:
            dialect: SQL dialect - "postgres" (default), "sqlite", or "mysql"
            with_types: If True, params are returned as (type_tag, value) tuples
                showing the exact sea_query Value variant used for binding.

        Returns:
            tuple: (sql_string, parameters)

        Examples:
            sql, params = User.objects.filter(age__gte=18).sql()
            sql, params = User.objects.filter(age__gte=18).sql(with_types=True)
            # params = [("BigInt", 18)]
        """
        query_ir = self.to_ir()
        ir_bytes = msgpack.packb(query_ir, default=_msgpack_encoder)
        result: tuple[str, list[Any]] = render_sql_debug(ir_bytes, dialect, with_types)
        return result

    def query(self) -> dict[str, Any]:
        """
        Return query IR (Intermediate Representation) for inspection.

        Returns:
            dict: Query IR structure that would be sent to Rust

        Examples:
            query_ir = User.objects.filter(age__gte=18).query()
            print(query_ir)
        """
        return self.to_ir()

    def explain(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
        analyze: bool = False,
        format: str = "text",
    ) -> Coroutine[Any, Any, str]:
        """
        Get query execution plan from the database.

        Args:
            using: Database alias to use
            client: Optional database client
            analyze: Whether to execute the query and show actual times
            format: Output format - "text" or "json"

        Returns:
            Query plan from database

        Examples:
            plan = await User.objects.filter(age__gte=18).explain()
            plan = await User.objects.filter(age__gte=18).explain(analyze=True)
        """

        async def runner() -> str:
            # Resolve pool name from using/client
            pool_name = _resolve_pool_name(using, client)

            query_ir = self.to_ir()
            ir_bytes = msgpack.packb(query_ir, default=_msgpack_encoder)

            # Call Rust explain function
            plan: str = await explain_query(
                pool_name, ir_bytes, analyze=analyze, format=format
            )
            return plan

        return runner()

    def union(self, other_query: DebugMixin) -> Self:
        """
        Combine with another query using UNION (removes duplicates).

        Args:
            other_query: Query to union with

        Examples:
            active = User.objects.filter(status="active")
            premium = User.objects.filter(status="premium")
            combined = active.union(premium)
        """
        clone = self._clone()
        clone._union_query = other_query
        clone._union_all = False
        return clone

    def union_all(self, other_query: DebugMixin) -> Self:
        """
        Combine with another query using UNION ALL (keeps duplicates).

        Args:
            other_query: Query to union with

        Examples:
            q1 = User.objects.filter(age__gte=18)
            q2 = User.objects.filter(status="premium")
            combined = q1.union_all(q2)
        """
        clone = self._clone()
        clone._union_query = other_query
        clone._union_all = True
        return clone

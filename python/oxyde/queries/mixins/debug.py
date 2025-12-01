"""Debug mixin for query building."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import msgpack

from oxyde.queries.base import SupportsExecute, TQuery, _resolve_pool_name

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


class DebugMixin:
    """Mixin providing debugging and introspection capabilities."""

    # These attributes are defined in the base Query class
    model_class: type[OxydeModel]
    _union_query: DebugMixin | None
    _union_all: bool

    def _clone(self: TQuery) -> TQuery:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def to_ir(self) -> dict[str, Any]:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def sql(self, *, dialect: str = "postgres") -> tuple[str, list[Any]]:
        """
        Return SQL representation and parameters for debugging.

        This calls Rust's render_sql_debug() to generate real SQL using the same
        logic that executes queries, ensuring consistency. Works without database
        connection.

        Args:
            dialect: SQL dialect - "postgres" (default), "sqlite", or "mysql"

        Returns:
            tuple: (sql_string, parameters) - real SQL that would be executed

        Examples:
            sql, params = User.objects.filter(age__gte=18).sql()
            print(f"SQL: {sql}")
            print(f"Params: {params}")
        """
        from datetime import datetime

        from oxyde.core.wrapper import render_sql_debug

        def _encoder(obj):
            if isinstance(obj, datetime):
                return obj.isoformat()
            return obj

        # Serialize query IR
        query_ir = self.to_ir()
        ir_bytes = msgpack.packb(query_ir, default=_encoder)

        # Call Rust render_sql_debug (synchronous, no DB required)
        return render_sql_debug(ir_bytes, dialect)

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
    ):
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
        from datetime import datetime

        from oxyde.core.wrapper import explain_query

        async def runner():
            # Resolve pool name from using/client
            pool_name = _resolve_pool_name(using, client)

            def _encoder(obj):
                if isinstance(obj, datetime):
                    return obj.isoformat()
                return obj

            query_ir = self.to_ir()
            ir_bytes = msgpack.packb(query_ir, default=_encoder)

            # Call Rust explain function
            plan = await explain_query(
                pool_name, ir_bytes, analyze=analyze, format=format
            )
            return plan

        return runner()

    def union(self: TQuery, other_query: DebugMixin) -> TQuery:
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

    def union_all(self: TQuery, other_query: DebugMixin) -> TQuery:
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

"""Mutation mixin for query building."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import msgpack

from oxyde.core import ir
from oxyde.queries.base import (
    SupportsExecute,
    _build_col_types,
    _map_values_to_columns,
    _model_key,
    _resolve_execution_client,
)
from oxyde.queries.expressions import F, _serialize_value_for_ir

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


class MutationMixin:
    """Mixin providing data mutation capabilities."""

    # These attributes are defined in the base Query class
    model_class: type[OxydeModel]

    def _build_filter_tree(self) -> ir.FilterNode | None:
        """Must be implemented by FilteringMixin."""
        raise NotImplementedError

    async def increment(
        self,
        field: str,
        by: int | float = 1,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> int:
        """
        Atomically increment a field value.

        Args:
            field: Field name to increment
            by: Amount to increment (default 1)
            using: Database alias
            client: Optional database client

        Returns:
            Number of affected rows

        Examples:
            await Post.objects.filter(id=42).increment("views", by=1)
            await User.objects.filter(is_active=True).increment("login_count")
        """
        exec_client = await _resolve_execution_client(using, client)
        col_types = _build_col_types(self.model_class)
        update_ir = ir.build_update_ir(
            table=self.model_class.get_table_name(),
            values={field: _serialize_value_for_ir(F(field) + by)},
            filter_tree=self._build_filter_tree(),
            col_types=col_types,
            model=_model_key(self.model_class),
        )
        result_bytes = await exec_client.execute(update_ir)
        result = msgpack.unpackb(result_bytes, raw=False)
        return result.get("affected", 0)

    async def update(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
        **values: Any,
    ) -> int:
        """
        Update records matching the query.

        Args:
            using: Database alias
            client: Optional database client
            **values: Field values to update

        Returns:
            Number of affected rows

        Examples:
            await Post.objects.filter(id=42).update(status="published")
            await User.objects.filter(is_active=False).update(status="archived")
        """
        exec_client = await _resolve_execution_client(using, client)
        col_types = _build_col_types(self.model_class)
        mapped_values = _map_values_to_columns(self.model_class, values)
        serialized_values = {
            key: _serialize_value_for_ir(value) for key, value in mapped_values.items()
        }
        update_ir = ir.build_update_ir(
            table=self.model_class.get_table_name(),
            values=serialized_values,
            filter_tree=self._build_filter_tree(),
            col_types=col_types,
            model=_model_key(self.model_class),
        )
        result_bytes = await exec_client.execute(update_ir)
        result = msgpack.unpackb(result_bytes, raw=False)
        return result.get("affected", 0)

    async def delete(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> int:
        """
        Delete records matching the query.

        Args:
            using: Database alias
            client: Optional database client

        Returns:
            Number of affected rows

        Examples:
            await Post.objects.filter(id=42).delete()
            await User.objects.filter(is_active=False).delete()
        """
        exec_client = await _resolve_execution_client(using, client)
        delete_ir = ir.build_delete_ir(
            table=self.model_class.get_table_name(),
            filter_tree=self._build_filter_tree(),
            model=_model_key(self.model_class),
        )
        result_bytes = await exec_client.execute(delete_ir)
        result = msgpack.unpackb(result_bytes, raw=False)
        return result.get("affected", 0)

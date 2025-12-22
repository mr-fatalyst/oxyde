"""INSERT query builder for single and bulk inserts (internal).

InsertQuery is used internally by QueryManager for create() and bulk_create().
Use Model.objects.create() and Model.objects.bulk_create() instead.

Example (via Manager):
    user = await User.objects.create(name="Alice", age=30)
    users = await User.objects.bulk_create([
        User(name="Alice", age=30),
        User(name="Bob", age=25),
    ])
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import msgpack

from oxyde.core import ir
from oxyde.queries.base import (
    SupportsExecute,
    _build_col_types,
    _map_values_to_columns,
    _model_key,
    _primary_key_meta,
)
from oxyde.queries.expressions import _serialize_value_for_ir

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


class InsertQuery:
    """INSERT query builder."""

    def __init__(self, model_class: type[OxydeModel]):
        self.model_class = model_class
        self._values: dict[str, Any] = {}
        self._bulk_values: list[dict[str, Any]] | None = None

    def _clone(self) -> InsertQuery:
        clone = InsertQuery(self.model_class)
        clone._values = dict(self._values)
        clone._bulk_values = list(self._bulk_values) if self._bulk_values else None
        return clone

    def values(self, **kwargs: Any) -> InsertQuery:
        """Set values to insert."""
        clone = self._clone()
        clone._values = dict(kwargs)
        clone._bulk_values = None
        return clone

    def bulk_values(self, values: list[dict[str, Any]]) -> InsertQuery:
        """Set bulk values for batch insert."""
        clone = self._clone()
        clone._bulk_values = list(values)
        clone._values = {}
        return clone

    def _get_pk_column(self) -> str | None:
        """Get the database column name for the primary key field."""
        try:
            pk_meta = _primary_key_meta(self.model_class)
            return pk_meta.db_column
        except Exception:
            return None

    def to_ir(self) -> dict[str, Any]:
        """Convert to IR format."""
        table_name = self.model_class.get_table_name()
        pk_column = self._get_pk_column()
        col_types = _build_col_types(self.model_class)

        if self._bulk_values:
            if not self._bulk_values:
                raise ValueError("Bulk INSERT requires at least one row")

            # Serialize all rows
            serialized_bulk = []
            for row_values in self._bulk_values:
                mapped = _map_values_to_columns(self.model_class, row_values)
                serialized = {
                    key: _serialize_value_for_ir(value) for key, value in mapped.items()
                }
                serialized_bulk.append(serialized)

            # Bulk insert: only return PKs for efficiency
            return ir.build_insert_ir(
                table=table_name,
                bulk_values=serialized_bulk,
                col_types=col_types,
                model=_model_key(self.model_class),
                pk_column=pk_column,
            )
        else:
            if not self._values:
                raise ValueError("INSERT query requires at least one column/value pair")

            mapped_values = _map_values_to_columns(self.model_class, self._values)
            serialized_values = {
                key: _serialize_value_for_ir(value)
                for key, value in mapped_values.items()
            }

            # Single insert: use RETURNING * to get all fields (including db defaults)
            return ir.build_insert_ir(
                table=table_name,
                values=serialized_values,
                col_types=col_types,
                model=_model_key(self.model_class),
                pk_column=pk_column,
                returning=True,
            )

    async def execute(self, client: SupportsExecute) -> dict[str, Any]:
        """Execute insert query."""
        ir = self.to_ir()
        result_bytes = await client.execute(ir)
        return msgpack.unpackb(result_bytes, raw=False)


__all__ = ["InsertQuery"]

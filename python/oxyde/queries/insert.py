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

from collections.abc import Iterable
from typing import TYPE_CHECKING, Any, Literal

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
    from oxyde.models.base import Model


class InsertQuery:
    """INSERT query builder."""

    def __init__(self, model_class: type[Model]):
        self.model_class = model_class
        self._values: dict[str, Any] = {}
        self._bulk_values: list[dict[str, Any]] | None = None
        self._on_conflict: dict[str, Any] | None = None
        self._returning: bool | None = None

    def _clone(self) -> InsertQuery:
        clone = InsertQuery(self.model_class)
        clone._values = dict(self._values)
        clone._bulk_values = list(self._bulk_values) if self._bulk_values else None
        clone._returning = self._returning
        clone._on_conflict = (
            {
                **self._on_conflict,
                "columns": list(self._on_conflict["columns"]),
                "update_values": (
                    dict(self._on_conflict["update_values"])
                    if self._on_conflict.get("update_values") is not None
                    else None
                ),
            }
            if self._on_conflict is not None
            else None
        )
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

    def returning(self, enabled: bool = True) -> InsertQuery:
        """Set whether the insert should request RETURNING rows."""
        clone = self._clone()
        clone._returning = enabled
        return clone

    def on_conflict(
        self,
        *,
        columns: Iterable[str],
        action: Literal["nothing", "update"],
        update_values: dict[str, Any] | None = None,
    ) -> InsertQuery:
        """Configure ON CONFLICT / ON DUPLICATE KEY behavior."""
        clone = self._clone()
        conflict_columns = list(columns)
        if not conflict_columns:
            raise ValueError("on_conflict requires at least one column")
        if action == "update":
            if not update_values:
                raise ValueError("on_conflict(update) requires update_values")
            clone._on_conflict = {
                "columns": conflict_columns,
                "action": action,
                "update_values": dict(update_values),
            }
        else:
            clone._on_conflict = {
                "columns": conflict_columns,
                "action": action,
            }
        return clone

    def _get_pk_column(self) -> str | None:
        """Get the database column name for the primary key field."""
        try:
            pk_meta = _primary_key_meta(self.model_class)
            return pk_meta.db_column
        except Exception:
            return None

    def _serialize_on_conflict(self) -> dict[str, Any] | None:
        """Map ON CONFLICT fields through model metadata before building IR."""
        if self._on_conflict is None:
            return None

        metadata = self.model_class._db_meta.field_metadata
        columns = []
        for column in self._on_conflict["columns"]:
            meta = metadata.get(column)
            columns.append(meta.db_column if meta else column)
        serialized = {
            "columns": columns,
            "action": self._on_conflict["action"],
        }

        update_values = self._on_conflict.get("update_values")
        if update_values is not None:
            mapped_values = _map_values_to_columns(self.model_class, update_values)
            serialized["update_values"] = {
                key: _serialize_value_for_ir(value)
                for key, value in mapped_values.items()
            }

        return serialized

    def to_ir(self) -> dict[str, Any]:
        """Convert to IR format."""
        table_name = self.model_class.get_table_name()
        pk_column = self._get_pk_column()
        col_types = _build_col_types(self.model_class)
        on_conflict = self._serialize_on_conflict()

        if self._bulk_values is not None:
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
                on_conflict=on_conflict,
            )
        else:
            if not self._values:
                raise ValueError("INSERT query requires at least one column/value pair")

            mapped_values = _map_values_to_columns(self.model_class, self._values)
            serialized_values = {
                key: _serialize_value_for_ir(value)
                for key, value in mapped_values.items()
            }

            # Single-row inserts default to RETURNING * so create() hydrates db defaults.
            return ir.build_insert_ir(
                table=table_name,
                values=serialized_values,
                col_types=col_types,
                model=_model_key(self.model_class),
                pk_column=pk_column,
                returning=(self._returning if self._returning is not None else True),
                on_conflict=on_conflict,
            )

    async def execute(self, client: SupportsExecute) -> dict[str, Any]:
        """Execute insert query."""
        ir = self.to_ir()
        result_bytes = await client.execute(ir)
        return msgpack.unpackb(result_bytes, raw=False)


__all__ = ["InsertQuery"]

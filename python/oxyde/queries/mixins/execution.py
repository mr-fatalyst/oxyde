"""Execution mixin for query building."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import msgpack

from oxyde.core import ir
from oxyde.exceptions import LookupError
from oxyde.queries.base import (
    _TYPE_ADAPTER_CACHE,
    _TYPE_ADAPTER_LOCK,
    SupportsExecute,
    TQuery,
    _primary_key_meta,
    _resolve_execution_client,
    _resolve_registered_model,
)
from oxyde.queries.joins import _JoinDescriptor

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


class ExecutionMixin:
    """Mixin providing query execution capabilities."""

    # These attributes are defined in the base Query class
    model_class: type[OxydeModel]
    _result_mode: str | None
    _values_flat: bool
    _selected_fields: list[str] | None
    _join_specs: list[_JoinDescriptor]
    _prefetch_paths: list[str]
    _limit_value: int | None
    _offset_value: int | None
    _order_by_fields: list[tuple[str, str]]

    def _clone(self: TQuery) -> TQuery:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def to_ir(self) -> dict[str, Any]:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def limit(self: TQuery, value: int) -> TQuery:
        """Must be implemented by PaginationMixin."""
        raise NotImplementedError

    def _build_filter_tree(self) -> ir.FilterNode | None:
        """Must be implemented by FilteringMixin."""
        raise NotImplementedError

    def _join_specs_to_ir(self) -> list[dict[str, Any]]:
        """Must be implemented by JoiningMixin."""
        raise NotImplementedError

    async def fetch_all(self, client: SupportsExecute) -> list[dict[str, Any]]:
        """Execute query and return all results as dicts."""
        result_bytes = await self.fetch_msgpack(client)
        rows = msgpack.unpackb(result_bytes, raw=False)
        if self._result_mode == "dict":
            return rows
        if self._result_mode == "list":
            if not rows:
                return []
            fields = self._selected_fields or list(rows[0].keys())
            if self._values_flat:
                if len(fields) != 1:
                    raise ValueError(
                        "values_list(flat=True) requires exactly one field"
                    )
                column = fields[0]
                return [row.get(column) for row in rows]
            return [tuple(row.get(field) for field in fields) for row in rows]
        return rows

    async def fetch_one(self, client: SupportsExecute) -> dict[str, Any] | None:
        """Execute query and return first result as dict."""
        query = self.limit(1)
        results = await query.fetch_all(client)
        return results[0] if results else None

    async def fetch_msgpack(self, client: SupportsExecute) -> bytes:
        """Execute query and return raw MessagePack bytes."""
        query_ir = self.to_ir()
        return await client.execute(query_ir)

    async def fetch_models(self, client: SupportsExecute) -> list[OxydeModel]:
        """Execute query and return results as model instances."""
        result_bytes = await self.fetch_msgpack(client)
        rows = msgpack.unpackb(result_bytes, raw=False)
        self._sanitize_join_placeholders(rows)

        # Get or create cached TypeAdapter (thread-safe)
        model_class = self.model_class
        if model_class not in _TYPE_ADAPTER_CACHE:
            with _TYPE_ADAPTER_LOCK:
                # Double-check after acquiring lock
                if model_class not in _TYPE_ADAPTER_CACHE:
                    from pydantic import TypeAdapter

                    _TYPE_ADAPTER_CACHE[model_class] = TypeAdapter(list[model_class])

        adapter = _TYPE_ADAPTER_CACHE[model_class]
        models = adapter.validate_python(rows)
        if self._join_specs:
            self._hydrate_join_results(models, rows)
        if self._prefetch_paths:
            await self._run_prefetch(models, client)
        return models

    def _sanitize_join_placeholders(self, rows: list[dict[str, Any]]) -> None:
        """Remove join placeholder values from rows."""
        if not rows or not self._join_specs:
            return
        for row in rows:
            for spec in self._join_specs:
                if spec.attr_name in row:
                    row[spec.attr_name] = None

    def all(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Execute query and return results based on result mode."""
        return self._execute(using=using, client=client)

    async def first(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> OxydeModel | None:
        """
        Return the first result or None.

        Applies LIMIT 1 and returns a single model instance.

        Returns:
            Model instance or None if no results

        Examples:
            user = await User.objects.filter(is_active=True).first()
        """
        exec_client = await _resolve_execution_client(using, client)
        query = self.limit(1)
        results = await query.fetch_models(exec_client)
        return results[0] if results else None

    async def last(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> OxydeModel | None:
        """
        Return the last result or None.

        Reverses the current ordering (or orders by PK desc) and returns one result.

        Returns:
            Model instance or None if no results

        Examples:
            user = await User.objects.order_by("created_at").last()
        """
        exec_client = await _resolve_execution_client(using, client)
        query = self._clone()

        # Reverse existing order or default to -pk
        if query._order_by_fields:
            reversed_order = []
            for field, direction in query._order_by_fields:
                new_dir = "DESC" if direction == "ASC" else "ASC"
                reversed_order.append((field, new_dir))
            query._order_by_fields = reversed_order
        else:
            # No ordering specified - use primary key descending
            pk_meta = _primary_key_meta(self.model_class)
            if pk_meta:
                query._order_by_fields = [(pk_meta.name, "DESC")]

        query = query.limit(1)
        results = await query.fetch_models(exec_client)
        return results[0] if results else None

    async def exists(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> bool:
        """
        Check if any records match the query.

        Returns:
            bool: True if at least one record exists

        Examples:
            await User.objects.filter(age__gte=18).exists()
        """
        exec_client = await _resolve_execution_client(using, client)

        # Build minimal exists IR with LIMIT 1
        query_ir = ir.build_select_ir(
            table=self.model_class.get_table_name(),
            filter_tree=self._build_filter_tree(),
            joins=self._join_specs_to_ir() or None,
            limit=1,
            exists=True,
        )

        result_bytes = await exec_client.execute(query_ir)
        result = msgpack.unpackb(result_bytes, raw=False)

        # Result is [{exists: true/false}] or [[true/false]]
        if isinstance(result, list) and len(result) > 0:
            row = result[0]
            if isinstance(row, dict):
                # Get first value from dict
                return bool(next(iter(row.values()), False))
            if isinstance(row, (list, tuple)):
                return bool(row[0]) if row else False
            return bool(row)
        return False

    def _execute(
        self,
        *,
        using: str | None,
        client: SupportsExecute | None,
    ):
        """Internal execution dispatcher."""

        async def runner():
            exec_client = await _resolve_execution_client(using, client)
            if self._result_mode == "msgpack":
                return await self.fetch_msgpack(exec_client)
            if self._result_mode in {"dict", "list"}:
                return await self.fetch_all(exec_client)
            return await self.fetch_models(exec_client)

        return runner()

    # --- Join hydration methods ---

    def _hydrate_join_results(
        self,
        models: list[OxydeModel],
        rows: list[dict[str, Any]],
    ) -> None:
        """Hydrate joined relations on model instances."""
        if not models or not self._join_specs:
            return
        ordered_specs = sorted(
            self._join_specs,
            key=lambda spec: spec.path.count("__"),
        )
        for model, row in zip(models, rows):
            for spec in ordered_specs:
                parent = self._resolve_join_parent(model, spec.parent_path)
                if parent is None:
                    continue
                payload: dict[str, Any] = {}
                for field, _ in spec.columns:
                    key = f"{spec.result_prefix}__{field}"
                    if key in row:
                        payload[field] = row.get(key)
                    else:
                        payload[field] = None
                if all(value is None for value in payload.values()):
                    setattr(parent, spec.attr_name, None)
                    continue
                related = spec.target_model(**payload)
                setattr(parent, spec.attr_name, related)

    def _resolve_join_parent(
        self,
        model: OxydeModel,
        parent_path: str | None,
    ) -> OxydeModel | None:
        """Resolve parent model for nested join."""
        if not parent_path:
            return model
        current: Any = model
        for segment in parent_path.split("__"):
            current = getattr(current, segment, None)
            if current is None:
                return None
        return current

    # --- Prefetch methods ---

    async def _run_prefetch(
        self,
        parents: list[OxydeModel],
        client: SupportsExecute,
    ) -> None:
        """Run prefetch for all specified paths."""
        for path in self._prefetch_paths:
            segments = path.split("__")
            await self._prefetch_path(parents, client, segments, self.model_class)

    async def _prefetch_path(
        self,
        parents: list[OxydeModel],
        client: SupportsExecute,
        segments: list[str],
        current_model: type[OxydeModel],
    ) -> None:
        """Prefetch a single relation path."""
        if not parents:
            return
        relation_name = segments[0]
        relation = current_model._db_meta.relations.get(relation_name)
        if relation is None:
            raise LookupError(
                f"{current_model.__name__} has no relation '{relation_name}'"
            )
        if relation.kind != "one_to_many":
            raise LookupError(f"prefetch('{relation_name}') supports one-to-many only")
        if relation.remote_field is None:
            raise LookupError(
                f"Relation '{relation_name}' is missing a remote_field definition"
            )
        target_model = _resolve_registered_model(relation.target)
        parent_pk = _primary_key_meta(current_model)
        parent_ids = [
            getattr(parent, parent_pk.name)
            for parent in parents
            if getattr(parent, parent_pk.name, None) is not None
        ]
        unique_ids: list[Any] = []
        seen: set[Any] = set()
        for value in parent_ids:
            if value not in seen:
                seen.add(value)
                unique_ids.append(value)

        grouped: dict[Any, list[OxydeModel]] = {}
        if unique_ids:
            # Use Manager.filter() with __in lookup
            filter_kwargs = {f"{relation.remote_field}__in": unique_ids}
            children = await target_model.objects.filter(**filter_kwargs).all(
                client=client
            )
            for child in children:
                key = getattr(child, relation.remote_field, None)
                if key is None:
                    continue
                grouped.setdefault(key, []).append(child)

        descriptor = getattr(current_model, relation_name, None)

        for parent in parents:
            parent_id = getattr(parent, parent_pk.name, None)
            values = grouped.get(parent_id, [])
            if hasattr(descriptor, "__set__"):
                descriptor.__set__(parent, list(values))
            else:
                parent.__dict__[relation_name] = list(values)

        if len(segments) > 1:
            nested_children: list[OxydeModel] = [
                child for collection in grouped.values() for child in collection
            ]
            if nested_children:
                await self._prefetch_path(
                    nested_children,
                    client,
                    segments[1:],
                    target_model,
                )

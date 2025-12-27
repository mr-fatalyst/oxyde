"""Execution mixin for query building.

Provides fetch_models(), fetch_all(), fetch_one() and other execution methods.
JOIN fields (author__id, author__name) are handled via extra="ignore" in
OxydeModel.model_config - no sanitization needed.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import msgpack

from oxyde.core import ir
from oxyde.exceptions import FieldLookupError, MultipleObjectsReturned, NotFoundError
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
        data = msgpack.unpackb(result_bytes, raw=False)

        # Handle columnar format: (columns, rows) tuple from Rust
        # This is more memory-efficient than list of dicts
        if isinstance(data, (list, tuple)) and len(data) == 2:
            first, second = data
            if isinstance(first, list) and all(isinstance(c, str) for c in first):
                # Columnar format: first is column names, second is rows
                columns = first
                row_values = second
                # Convert to dicts via zip (fast C implementation)
                rows = [dict(zip(columns, row)) for row in row_values]
            else:
                # Old format: list of dicts
                rows = data
        else:
            # Old format or empty
            rows = data if isinstance(data, list) else []

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

    async def fetch_rows(self, client: SupportsExecute) -> list[dict[str, Any]]:
        """Execute query and return rows as list[dict].

        Uses direct msgpack path (no batching) for maximum speed on simple queries.
        """
        result_bytes = await self.fetch_msgpack(client)
        data = msgpack.unpackb(result_bytes, raw=False)
        if isinstance(data, (list, tuple)) and len(data) == 2:
            first, second = data
            if isinstance(first, list) and all(isinstance(c, str) for c in first):
                return [dict(zip(first, row)) for row in second]
        return data if isinstance(data, list) else []

    async def fetch_models(self, client: SupportsExecute) -> list[OxydeModel]:
        """Execute query and return results as model instances.

        Uses direct PyDict path (no msgpack) when available.
        JOIN fields (author__id, author__name) are extracted and hydrated.
        """
        # Get or create cached TypeAdapter (thread-safe)
        model_class = self.model_class
        if model_class not in _TYPE_ADAPTER_CACHE:
            with _TYPE_ADAPTER_LOCK:
                # Double-check after acquiring lock
                if model_class not in _TYPE_ADAPTER_CACHE:
                    from pydantic import TypeAdapter

                    _TYPE_ADAPTER_CACHE[model_class] = TypeAdapter(list[model_class])

        adapter = _TYPE_ADAPTER_CACHE[model_class]

        # Use dedup batched execution for JOIN queries (saves ~38% memory)
        if self._join_specs and hasattr(client, "execute_batched_dedup"):
            result = await client.execute_batched_dedup(self.to_ir())
            if result:
                main_rows = result.get("main", [])
                relations = result.get("relations", {})

                # Validate main models
                models = adapter.validate_python(main_rows)

                # Hydrate relations from dedup format
                if models and relations:
                    self._hydrate_from_dedup(models, main_rows, relations)

                if self._prefetch_paths:
                    await self._run_prefetch(models, client)
                return models
            return []

        # Fallback to regular fetch_rows for non-JOIN queries
        rows = await self.fetch_rows(client)

        # Validate main models (Pydantic ignores extra fields like author__id)
        models = adapter.validate_python(rows)

        # Hydrate JOIN relations if present (old path)
        if self._join_specs and rows:
            self._hydrate_from_columnar(models, rows)

        if self._prefetch_paths:
            await self._run_prefetch(models, client)
        return models

    def all(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Execute query and return results as Pydantic models.

        Args:
            using: Name of connection from registry
            client: Explicit client for execution

        Returns:
            list[Model] - Pydantic model instances
        """
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

        # Handle columnar format: (columns, rows)
        if isinstance(result, (list, tuple)) and len(result) == 2:
            first, second = result
            if isinstance(first, list) and all(isinstance(c, str) for c in first):
                # Columnar format
                rows = second
                if rows:
                    return bool(rows[0][0]) if rows[0] else False
                return False

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

    async def get(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> OxydeModel:
        """
        Return exactly one result matching the query.

        Raises NotFoundError if no match, MultipleObjectsReturned if more than one.

        Returns:
            Model instance

        Examples:
            user = await User.objects.filter(id=1).get()
        """
        exec_client = await _resolve_execution_client(using, client)
        query = self.limit(2)
        results = await query.fetch_models(exec_client)
        if not results:
            raise NotFoundError(f"{self.model_class.__name__} matching query not found")
        if len(results) > 1:
            raise MultipleObjectsReturned(
                f"Query for {self.model_class.__name__} returned multiple objects"
            )
        return results[0]

    async def get_or_none(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> OxydeModel | None:
        """
        Return one result or None if not found.

        Raises MultipleObjectsReturned if more than one match.

        Returns:
            Model instance or None

        Examples:
            user = await User.objects.filter(email="test@example.com").get_or_none()
        """
        try:
            return await self.get(using=using, client=client)
        except NotFoundError:
            return None

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

    def _hydrate_from_columnar(
        self,
        models: list[OxydeModel],
        rows: list[dict[str, Any]],
    ) -> None:
        """Hydrate joined relations from columnar format.

        Extracts related model data from prefixed columns (e.g., author__id)
        and creates related model instances, caching by PK to avoid duplicates.

        Args:
            models: Main model instances (already validated)
            rows: Row dicts with both main and related columns
        """
        if not models or not self._join_specs:
            return

        # Sort specs by depth (shallow first) for nested joins
        ordered_specs = sorted(
            self._join_specs,
            key=lambda spec: spec.path.count("__"),
        )

        # Pre-compute prefixes and pk columns for each spec (avoid repeated string ops)
        spec_meta = [
            (spec, spec.result_prefix + "__", spec.result_prefix + "__id")
            for spec in ordered_specs
        ]

        # Pre-compute which columns belong to which spec (do once, not per row)
        # This avoids O(rows * columns * specs) startswith checks
        first_row = rows[0] if rows else {}
        spec_columns: dict[str, list[str]] = {}
        for spec, prefix, _ in spec_meta:
            cols = [k for k in first_row.keys() if k.startswith(prefix)]
            spec_columns[spec.path] = cols

        # Cache related models by (relation_path, pk) to reuse instances
        related_cache: dict[tuple[str, Any], OxydeModel] = {}

        # Hydrate each model from its corresponding row
        for model, row in zip(models, rows):
            for spec, prefix, pk_col in spec_meta:
                pk_value = row.get(pk_col)
                cache_key = (spec.path, pk_value)

                # Check cache first - avoid creating duplicate instances
                if pk_value is not None and cache_key in related_cache:
                    parent = self._resolve_join_parent(model, spec.parent_path)
                    if parent is not None:
                        setattr(parent, spec.attr_name, related_cache[cache_key])
                    continue

                # Extract related model fields using pre-computed column list
                cols = spec_columns[spec.path]
                prefix_len = len(prefix)
                related_data = {col[prefix_len:]: row[col] for col in cols}
                has_data = any(v is not None for v in related_data.values())

                # Find parent model for nested joins
                parent = self._resolve_join_parent(model, spec.parent_path)
                if parent is None:
                    continue

                # Create related instance or set None
                if has_data:
                    related_instance = spec.target_model(**related_data)
                    if pk_value is not None:
                        related_cache[cache_key] = related_instance
                    setattr(parent, spec.attr_name, related_instance)
                else:
                    setattr(parent, spec.attr_name, None)

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

    def _hydrate_from_dedup(
        self,
        models: list[OxydeModel],
        rows: list[dict[str, Any]],
        relations: dict[str, dict[Any, dict[str, Any]]],
    ) -> None:
        """Hydrate joined relations from dedup format.

        The dedup format stores each related entity once:
        relations = {"user": {1: {"id": 1, "name": "Alice"}, 2: {...}}}

        Args:
            models: Main model instances (already validated)
            rows: Main rows (contain FK columns like user_id)
            relations: Dict of relation_path -> {pk: related_data}
        """
        if not models or not self._join_specs:
            return

        # Sort specs by depth (shallow first) for nested joins
        ordered_specs = sorted(
            self._join_specs,
            key=lambda spec: spec.path.count("__"),
        )

        # Cache created model instances to reuse same object
        related_cache: dict[tuple[str, Any], OxydeModel] = {}

        for model, row in zip(models, rows):
            for spec in ordered_specs:
                path = spec.path
                rel_data = relations.get(path, {})

                # Get FK value from main row
                fk_col = spec.source_column  # e.g., "user_id"
                pk_value = row.get(fk_col)

                if pk_value is None:
                    # Null FK - set relation to None
                    parent = self._resolve_join_parent(model, spec.parent_path)
                    if parent is not None:
                        setattr(parent, spec.attr_name, None)
                    continue

                cache_key = (path, pk_value)

                # Check if we already created this instance
                if cache_key in related_cache:
                    parent = self._resolve_join_parent(model, spec.parent_path)
                    if parent is not None:
                        setattr(parent, spec.attr_name, related_cache[cache_key])
                    continue

                # Get related data from dedup dict
                related_row = rel_data.get(pk_value)
                if related_row:
                    # Create related model instance
                    related_instance = spec.target_model(**related_row)
                    related_cache[cache_key] = related_instance

                    parent = self._resolve_join_parent(model, spec.parent_path)
                    if parent is not None:
                        setattr(parent, spec.attr_name, related_instance)
                else:
                    # No data found for this PK
                    parent = self._resolve_join_parent(model, spec.parent_path)
                    if parent is not None:
                        setattr(parent, spec.attr_name, None)

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
            raise FieldLookupError(
                f"{current_model.__name__} has no relation '{relation_name}'"
            )

        # Handle many_to_many separately
        if relation.kind == "many_to_many":
            await self._prefetch_m2m(
                parents, client, relation, current_model, relation_name
            )
            # Handle nested prefetch for M2M
            if len(segments) > 1:
                target_model = _resolve_registered_model(relation.target)
                all_targets: list[OxydeModel] = []
                for parent in parents:
                    targets = getattr(parent, relation_name, [])
                    all_targets.extend(targets)
                if all_targets:
                    await self._prefetch_path(
                        all_targets, client, segments[1:], target_model
                    )
            return

        # one_to_many handling
        if relation.kind != "one_to_many":
            raise FieldLookupError(
                f"prefetch('{relation_name}') supports one-to-many and many-to-many only"
            )
        if relation.remote_field is None:
            raise FieldLookupError(
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
            # Determine FK column: if remote_field is a virtual FK, use its db_column
            target_model.ensure_field_metadata()
            fk_meta = target_model._db_meta.field_metadata.get(relation.remote_field)
            if fk_meta and fk_meta.foreign_key:
                fk_column = fk_meta.db_column  # e.g., "post_id"
            else:
                fk_column = relation.remote_field  # fallback for explicit columns

            # Use Manager.filter() with __in lookup
            filter_kwargs = {f"{fk_column}__in": unique_ids}
            children = await target_model.objects.filter(**filter_kwargs).all(
                client=client
            )
            for child in children:
                key = getattr(child, fk_column, None)
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

    async def _prefetch_m2m(
        self,
        parents: list[OxydeModel],
        client: SupportsExecute,
        relation: Any,  # RelationInfo
        source_model: type[OxydeModel],
        relation_name: str,
    ) -> None:
        """Prefetch many-to-many relation."""
        from oxyde.models.registry import registered_tables

        if not parents or not relation.through:
            return

        # Find through model in registry
        through_model: type[OxydeModel] | None = None
        tables = registered_tables()
        for key, model in tables.items():
            if (
                key.endswith(f".{relation.through}")
                or model.__name__ == relation.through
            ):
                through_model = model
                break

        if through_model is None:
            raise FieldLookupError(
                f"Through model '{relation.through}' not found in registry"
            )

        through_model.ensure_field_metadata()
        target_model = _resolve_registered_model(relation.target)

        # Find FK fields in through model
        source_fk_column: str | None = None
        target_fk_column: str | None = None
        source_key = f".{source_model.__name__}"
        target_key = f".{target_model.__name__}"

        for meta in through_model._db_meta.field_metadata.values():
            if meta.foreign_key:
                if meta.foreign_key.target.endswith(source_key):
                    source_fk_column = meta.foreign_key.column_name
                elif meta.foreign_key.target.endswith(target_key):
                    target_fk_column = meta.foreign_key.column_name

        if source_fk_column is None or target_fk_column is None:
            raise FieldLookupError(
                f"Through model '{relation.through}' must have FK fields "
                f"to both '{source_model.__name__}' and '{target_model.__name__}'"
            )

        # Collect parent IDs
        parent_pk = _primary_key_meta(source_model)
        parent_ids = [
            getattr(parent, parent_pk.name)
            for parent in parents
            if getattr(parent, parent_pk.name, None) is not None
        ]
        if not parent_ids:
            return

        # Deduplicate
        unique_ids: list[Any] = []
        seen: set[Any] = set()
        for value in parent_ids:
            if value not in seen:
                seen.add(value)
                unique_ids.append(value)

        # Query through table for links
        # Use synthetic FK column names (e.g., "post_id", "tag_id") not relation names
        filter_kwargs = {f"{source_fk_column}__in": unique_ids}
        links = await through_model.objects.filter(**filter_kwargs).all(client=client)

        # Collect target IDs from links using FK column names
        target_ids: list[Any] = []
        for link in links:
            target_id = getattr(link, target_fk_column, None)
            if target_id is not None and target_id not in target_ids:
                target_ids.append(target_id)

        # Query target model
        target_pk = _primary_key_meta(target_model)
        targets_by_pk: dict[Any, OxydeModel] = {}
        if target_ids:
            filter_kwargs = {f"{target_pk.name}__in": target_ids}
            targets = await target_model.objects.filter(**filter_kwargs).all(
                client=client
            )
            for target in targets:
                pk_val = getattr(target, target_pk.name)
                targets_by_pk[pk_val] = target

        # Group targets by source ID using FK column names
        grouped: dict[Any, list[OxydeModel]] = {}
        for link in links:
            source_id = getattr(link, source_fk_column, None)
            target_id = getattr(link, target_fk_column, None)
            if source_id is not None and target_id in targets_by_pk:
                grouped.setdefault(source_id, []).append(targets_by_pk[target_id])

        # Assign to parents
        descriptor = getattr(source_model, relation_name, None)
        for parent in parents:
            parent_id = getattr(parent, parent_pk.name, None)
            values = grouped.get(parent_id, [])
            if hasattr(descriptor, "__set__"):
                descriptor.__set__(parent, list(values))
            else:
                parent.__dict__[relation_name] = list(values)

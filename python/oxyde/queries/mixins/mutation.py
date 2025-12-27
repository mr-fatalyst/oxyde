"""Mutation mixin for query building."""

from __future__ import annotations

import warnings
from collections.abc import Iterable
from typing import TYPE_CHECKING, Any

import msgpack

from oxyde.core import ir
from oxyde.exceptions import IntegrityError, ManagerError
from oxyde.models.serializers import (
    _dump_insert_data,
    _normalize_instance,
)
from oxyde.queries.base import (
    SupportsExecute,
    _build_col_types,
    _map_values_to_columns,
    _model_key,
    _resolve_execution_client,
)
from oxyde.queries.expressions import F, _serialize_value_for_ir
from oxyde.queries.insert import InsertQuery

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
    ) -> list[dict[str, Any]]:
        """
        Update records matching the query.

        Args:
            using: Database alias
            client: Optional database client
            **values: Field values to update

        Returns:
            List of updated rows as dicts (with all fields from RETURNING *)

        Examples:
            rows = await Post.objects.filter(id=42).update(status="published")
            rows = await User.objects.filter(is_active=False).update(status="archived")
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
            returning=True,
        )
        result_bytes = await exec_client.execute(update_ir)
        result = msgpack.unpackb(result_bytes, raw=False)
        # Convert columnar format to list of dicts
        columns = result.get("columns", [])
        rows = result.get("rows", [])
        return [dict(zip(columns, row)) for row in rows]

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

    def _primary_key_field(self) -> str | None:
        """Get primary key field name."""
        self.model_class.ensure_field_metadata()
        for field_name, meta in self.model_class._db_meta.field_metadata.items():
            if meta.primary_key:
                return field_name
        return None

    async def _run_mutation(
        self, query: Any, client: SupportsExecute
    ) -> dict[str, Any]:
        """Execute mutation query with error handling."""
        try:
            result = await query.execute(client)
        except ManagerError:
            raise
        except Exception as exc:  # pragma: no cover - driver specific issues
            message = str(exc)
            if "constraint" in message.lower():
                raise IntegrityError(message) from exc
            raise ManagerError(message) from exc
        if not isinstance(result, dict):
            raise ManagerError("Mutation response must be a dict")
        return result

    async def create(
        self,
        *,
        instance: OxydeModel | None = None,
        using: str | None = None,
        client: SupportsExecute | None = None,
        _skip_hooks: bool = False,
        **data: Any,
    ) -> OxydeModel:
        """
        Create a new record in the database.

        Args:
            instance: Model instance to create (alternative to **data)
            using: Database alias
            client: Optional database client
            _skip_hooks: Skip pre_save/post_save hooks
            **data: Field values for the new record

        Returns:
            Created model instance with populated PK

        Examples:
            user = await User.objects.create(name="Alice", email="alice@example.com")
            # Or with instance:
            user = User(name="Alice")
            user = await User.objects.create(instance=user)
        """
        if instance is not None and data:
            raise ManagerError(
                "create() accepts either 'instance' or field values, not both"
            )
        if instance is None:
            if not data:
                raise ManagerError("create() requires an instance or field values")
            instance = self.model_class(**data)

        # Call pre_save hook
        if not _skip_hooks:
            await instance.pre_save(is_create=True, update_fields=None)

        exec_client = await _resolve_execution_client(using, client)
        payload = _dump_insert_data(instance)
        if not payload:
            raise ManagerError("create() requires at least one value")
        query = InsertQuery(self.model_class).values(**payload)
        result = await self._run_mutation(query, exec_client)

        # Update instance from RETURNING * result
        if "rows" in result and result["rows"]:
            # Build db_column -> field_name mapping
            self.model_class.ensure_field_metadata()
            col_to_field = {
                meta.db_column: field_name
                for field_name, meta in self.model_class._db_meta.field_metadata.items()
            }
            # Update instance with returned values
            columns = result.get("columns", [])
            row = result["rows"][0]
            for col, value in zip(columns, row):
                field_name = col_to_field.get(col, col)
                setattr(instance, field_name, value)
        elif "inserted_ids" in result and result["inserted_ids"]:
            # Fallback for bulk insert (only PKs returned)
            pk_field = self._primary_key_field()
            if pk_field:
                setattr(instance, pk_field, result["inserted_ids"][0])

        # Call post_save hook
        if not _skip_hooks:
            await instance.post_save(is_create=True, update_fields=None)

        return instance

    async def bulk_create(
        self,
        objects: Iterable[Any],
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
        batch_size: int | None = None,
    ) -> list[OxydeModel]:
        """
        Insert multiple objects efficiently.

        Args:
            objects: Iterable of model instances or dicts
            using: Database alias
            client: Optional database client
            batch_size: Optional batch size. If None, inserts all in one query.
                Use if hitting DB param limits (SQLite: 999, Postgres: 65535).

        Returns:
            List of created model instances
        """
        instances = [_normalize_instance(self.model_class, obj) for obj in objects]
        if not instances:
            return []

        exec_client = await _resolve_execution_client(using, client)

        # Determine batches: all at once or user-specified batch_size
        if batch_size is None:
            batches = [instances]
        else:
            batches = [
                instances[i : i + batch_size]
                for i in range(0, len(instances), batch_size)
            ]

        for batch in batches:
            payloads = []
            for instance in batch:
                payload = _dump_insert_data(instance)
                if not payload:
                    raise ManagerError("bulk_create() encountered an empty payload")
                payloads.append(payload)

            query = InsertQuery(self.model_class).bulk_values(payloads)
            result = await self._run_mutation(query, exec_client)

            # Assign auto-generated PKs to instances
            inserted_ids = result.get("inserted_ids", [])
            pk_field = self._primary_key_field()

            # Warn if ID count doesn't match (MySQL limitation)
            if inserted_ids and len(inserted_ids) != len(batch):
                warnings.warn(
                    f"bulk_create: received {len(inserted_ids)} IDs for {len(batch)} rows. "
                    "This may occur with MySQL when using ON DUPLICATE KEY, "
                    "non-sequential auto_increment, or non-integer primary keys. "
                    "Assigned IDs may be incorrect.",
                    RuntimeWarning,
                    stacklevel=2,
                )

            if pk_field and inserted_ids:
                for instance, pk_value in zip(batch, inserted_ids):
                    setattr(instance, pk_field, pk_value)

        return instances

    async def bulk_update(
        self,
        objects: Iterable[OxydeModel],
        fields: Iterable[str],
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> int:
        """
        Bulk update multiple objects with CASE WHEN.

        Args:
            objects: Instances to update
            fields: Field names to update
            using: Database alias
            client: Optional database client

        Returns:
            Number of affected rows

        Examples:
            users = await User.objects.filter(is_active=True).all()
            for user in users:
                user.login_count += 1
            await User.objects.bulk_update(users, ["login_count"])
        """
        objects_list = list(objects)
        if not objects_list:
            return 0

        fields_list = list(fields)
        if not fields_list:
            raise ManagerError("bulk_update() requires at least one field")

        pk_field = self._primary_key_field()
        if not pk_field:
            raise ManagerError("bulk_update() requires a primary key")

        exec_client = await _resolve_execution_client(using, client)

        # Build bulk_update payload
        bulk_entries: list[dict[str, Any]] = []
        for obj in objects_list:
            pk_value = getattr(obj, pk_field)
            if pk_value is None:
                continue

            # Use model_dump to serialize fields (respects @field_serializer)
            # mode='json' gives JSON values (datetime -> ISO string, etc.)
            all_values = obj.model_dump(mode="json", exclude_none=False)

            # Extract only requested fields
            values = {}
            for field_name in fields_list:
                if field_name in all_values:
                    values[field_name] = all_values[field_name]

            if values:
                bulk_entries.append(
                    {
                        "filters": {pk_field: pk_value},
                        "values": values,
                    }
                )

        if not bulk_entries:
            return 0

        col_types = _build_col_types(self.model_class)
        update_ir = ir.build_update_ir(
            table=self.model_class.get_table_name(),
            bulk_update=bulk_entries,
            col_types=col_types,
            model=_model_key(self.model_class),
        )

        result_bytes = await exec_client.execute(update_ir)
        result = msgpack.unpackb(result_bytes, raw=False)
        return int(result.get("affected", 0))

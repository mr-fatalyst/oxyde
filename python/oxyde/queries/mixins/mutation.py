"""Mutation mixin for query building."""

from __future__ import annotations

import warnings
from collections.abc import Iterable
from typing import TYPE_CHECKING, Any, Literal, overload

import msgpack

from oxyde.core import ir
from oxyde.db.registry import get_connection
from oxyde.exceptions import IntegrityError, ManagerError
from oxyde.models.serializers import (
    _dump_insert_data,
    _normalize_instance,
)
from oxyde.queries.base import (
    SupportsExecute,
    _build_col_types,
    _collect_model_columns,
    _map_values_to_columns,
    _model_key,
    _resolve_execution_client,
)
from oxyde.queries.expressions import F, _serialize_value_for_ir
from oxyde.queries.insert import InsertQuery

if TYPE_CHECKING:
    from oxyde.models.base import Model


async def _is_mysql(using: str | None, client: SupportsExecute | None) -> bool:
    """Check if the target database is MySQL via cached backend from Rust."""
    if client is not None:
        # AsyncTransaction → ._database, AsyncDatabase → direct
        db = getattr(client, "_database", client)
        return getattr(db, "backend", None) == "mysql"
    alias = using or "default"
    db = await get_connection(alias, ensure_connected=False)
    return db.backend == "mysql"


def _hydrate_models(
    model_class: type[Model], columns: list[str], rows: list[list[Any]]
) -> list[Model]:
    """Convert raw columns + rows into validated model instances."""
    rmap = model_class._db_meta.reverse_column_map
    field_columns = [rmap.get(c, c) for c in columns] if rmap else columns
    return [model_class.model_validate(dict(zip(field_columns, row))) for row in rows]


def _decode_returning_models(
    model_class: type[Model], result: dict[str, Any]
) -> list[Model]:
    """Decode mutation-returning result {columns, rows} into model instances."""
    rows = result.get("rows", [])
    if not rows:
        return []
    return _hydrate_models(model_class, result.get("columns", []), rows)


def _decode_columnar_models(model_class: type[Model], result: list[Any]) -> list[Model]:
    """Decode columnar SELECT result [columns, rows] into model instances."""
    if len(result) < 2 or not result[1]:
        return []
    return _hydrate_models(model_class, result[0], result[1])


class MutationMixin:
    """Mixin providing data mutation capabilities."""

    # These attributes are defined in the base Query class
    model_class: type[Model]

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

    @overload
    async def update(
        self,
        *,
        returning: Literal[True],
        using: str | None = ...,
        client: SupportsExecute | None = ...,
        **values: Any,
    ) -> list[Model]: ...

    @overload
    async def update(
        self,
        *,
        returning: Literal[False] = ...,
        using: str | None = ...,
        client: SupportsExecute | None = ...,
        **values: Any,
    ) -> int: ...

    async def update(
        self,
        *,
        returning: bool = False,
        using: str | None = None,
        client: SupportsExecute | None = None,
        **values: Any,
    ) -> int | list[Model]:
        """
        Update records matching the query.

        Args:
            returning: If True, return updated model instances.
                Default False — returns number of affected rows (Django-compatible).
            using: Database alias
            client: Optional database client
            **values: Field values to update

        Returns:
            Number of affected rows (default), or list of updated model instances
            if returning=True.

        Examples:
            count = await Post.objects.filter(id=42).update(status="published")
            posts = await Post.objects.filter(id=42).update(
                status="published", returning=True
            )
        """
        exec_client = await _resolve_execution_client(using, client)
        col_types = _build_col_types(self.model_class)
        mapped_values = _map_values_to_columns(self.model_class, values)
        serialized_values = {
            key: _serialize_value_for_ir(value) for key, value in mapped_values.items()
        }

        if returning and await _is_mysql(using, client):
            return await self._mysql_update_returning(
                using, client, exec_client, serialized_values, col_types
            )

        update_ir = ir.build_update_ir(
            table=self.model_class.get_table_name(),
            values=serialized_values,
            filter_tree=self._build_filter_tree(),
            col_types=col_types,
            model=_model_key(self.model_class),
            returning=returning,
        )
        result_bytes = await exec_client.execute(update_ir)
        result = msgpack.unpackb(result_bytes, raw=False)
        if returning:
            return _decode_returning_models(self.model_class, result)
        return result.get("affected", 0)

    async def _mysql_update_returning(
        self,
        using: str | None,
        client: SupportsExecute | None,
        exec_client: SupportsExecute,
        serialized_values: dict[str, Any],
        col_types: dict[str, str] | None,
    ) -> list[Model]:
        """MySQL fallback: SELECT PKs FOR UPDATE → UPDATE → re-fetch by PKs."""
        from oxyde.db.transaction import atomic, get_active_transaction

        pk_field = self.model_class._db_meta.pk_field
        if pk_field is None:
            raise ManagerError("update(returning=True) requires a primary key")
        meta = self.model_class._db_meta.field_metadata[pk_field]
        pk_db_col: str = meta.db_column or pk_field
        table = self.model_class.get_table_name()
        filter_tree = self._build_filter_tree()
        alias = using or "default"

        async def _do(tx_client: SupportsExecute) -> list[Model]:
            # 1. Collect PKs with FOR UPDATE lock
            select_pks_ir = ir.build_select_ir(
                table=table,
                columns=[pk_db_col],
                filter_tree=filter_tree,
                col_types=col_types,
                lock="update",
            )
            pk_bytes = await tx_client.execute(select_pks_ir)
            # Columnar format: [columns_list, rows_list]
            pk_result = msgpack.unpackb(pk_bytes, raw=False)
            pk_rows = pk_result[1] if len(pk_result) >= 2 else []
            pk_values = [row[0] for row in pk_rows]

            if not pk_values:
                return []

            # 2. Execute UPDATE (no returning)
            update_ir = ir.build_update_ir(
                table=table,
                values=serialized_values,
                filter_tree=filter_tree,
                col_types=col_types,
                model=_model_key(self.model_class),
                returning=False,
            )
            await tx_client.execute(update_ir)

            # 3. Re-fetch updated rows by PKs
            pk_in_filter: ir.FilterNode = {
                "type": "condition",
                "field": pk_db_col,
                "operator": "IN",
                "value": pk_values,
            }
            all_db_cols = [
                db_col for _, db_col in _collect_model_columns(self.model_class)
            ]
            refetch_ir = ir.build_select_ir(
                table=table,
                columns=all_db_cols,
                filter_tree=pk_in_filter,
                col_types=col_types,
            )
            refetch_bytes = await tx_client.execute(refetch_ir)
            # Columnar format: [columns_list, rows_list]
            refetch_result = msgpack.unpackb(refetch_bytes, raw=False)
            return _decode_columnar_models(self.model_class, refetch_result)

        # Use existing transaction or create an implicit one
        already_in_tx = client is not None or get_active_transaction(alias) is not None
        if already_in_tx:
            return await _do(exec_client)

        async with atomic(using=alias) as tx:
            return await _do(tx)

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
            col_types=_build_col_types(self.model_class),
            model=_model_key(self.model_class),
        )
        result_bytes = await exec_client.execute(delete_ir)
        result = msgpack.unpackb(result_bytes, raw=False)
        return result.get("affected", 0)

    def _primary_key_field(self) -> str | None:
        """Get primary key field name."""
        return self.model_class._db_meta.pk_field

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
        instance: Model | None = None,
        using: str | None = None,
        client: SupportsExecute | None = None,
        _skip_hooks: bool = False,
        **data: Any,
    ) -> Model:
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

        # Update instance from RETURNING * result (with Pydantic validation)
        if "rows" in result and result["rows"]:
            rmap = self.model_class._db_meta.reverse_column_map
            columns = result.get("columns", [])
            row = result["rows"][0]
            row_dict = {rmap.get(col, col): value for col, value in zip(columns, row)}
            instance = self.model_class.model_validate(row_dict)
        elif "inserted_ids" in result and result["inserted_ids"]:
            pk_field = self._primary_key_field()
            if pk_field:
                row_dict = instance.model_dump()
                row_dict[pk_field] = result["inserted_ids"][0]
                instance = self.model_class.model_validate(row_dict)

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
    ) -> list[Model]:
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
        if batch_size is not None and batch_size <= 0:
            raise ValueError("batch_size must be a positive integer")
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
        objects: Iterable[Model],
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

        pk_column = self.model_class._db_meta.field_metadata[pk_field].db_column

        exec_client = await _resolve_execution_client(using, client)

        # Build bulk_update payload
        bulk_entries: list[dict[str, Any]] = []
        for obj in objects_list:
            pk_value = getattr(obj, pk_field)
            if pk_value is None:
                continue

            all_values = obj.model_dump(mode="python", exclude_none=False)

            # Extract requested fields and map to db_column names
            values = {
                field_name: _serialize_value_for_ir(all_values[field_name])
                for field_name in fields_list
                if field_name in all_values
            }
            mapped_values = _map_values_to_columns(self.model_class, values)

            if mapped_values:
                bulk_entries.append(
                    {
                        "filters": {pk_column: pk_value},
                        "values": mapped_values,
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

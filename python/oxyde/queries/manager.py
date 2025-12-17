"""Django-style QueryManager for Model.objects interface.

This module provides QueryManager - the class behind Model.objects that
enables Django-like query syntax. Each OxydeModel gets a QueryManager
instance automatically assigned to its `objects` attribute.

Design:
    QueryManager wraps Query builder with async execution methods.
    Synchronous methods (filter, exclude, etc.) return Query objects.
    Async methods (all, get, create, etc.) execute immediately.

Methods - Query Building (sync, return Query):
    filter(*Q, **lookups): Add WHERE conditions
    exclude(*Q, **lookups): Add negated WHERE conditions
    values(*fields): Return dicts instead of models
    values_list(*fields, flat=False): Return tuples/lists
    distinct(): Add DISTINCT
    join(*paths): Eager load relations
    prefetch(*paths): Prefetch relations
    for_update(): Add FOR UPDATE lock
    for_share(): Add FOR SHARE lock

Methods - Execution (async, run query):
    all(using=None): Fetch all rows as models
    get(**filters): Fetch exactly one row (raises NotFoundError/MultipleObjectsReturned)
    get_or_none(**filters): Fetch one or return None
    first(): Fetch first row ordered by PK
    last(): Fetch last row ordered by PK DESC
    count(): COUNT(*) query
    sum/avg/max/min(field): Aggregate functions
    create(**data): INSERT and return model
    bulk_create(objects): INSERT multiple rows
    bulk_update(objects, fields): UPDATE multiple rows with CASE WHEN
    get_or_create(defaults=None, **filters): GET or INSERT

Client Resolution:
    _resolve_client() determines which connection to use:
    1. Explicit client parameter
    2. Active transaction (via get_active_transaction)
    3. Named connection (via using="alias")
    4. Default connection

Example:
    # Manager is auto-attached to models
    class User(OxydeModel):
        ...

    # Sync methods return Query
    query = User.objects.filter(active=True).order_by("name")

    # Async methods execute
    users = await User.objects.all()
    user = await User.objects.get(id=1)
    new_user = await User.objects.create(name="Alice")
"""

from __future__ import annotations

import warnings
from collections.abc import Iterable
from typing import Any

from oxyde.exceptions import (
    IntegrityError,
    ManagerError,
    MultipleObjectsReturned,
    NotFoundError,
)
from oxyde.models.base import OxydeModel
from oxyde.models.serializers import (
    _derive_create_data,
    _dump_insert_data,
    _normalize_instance,
)
from oxyde.queries.base import SupportsExecute, _resolve_execution_client
from oxyde.queries.insert import InsertQuery
from oxyde.queries.select import Query


class _QueryManagerBase:
    """Synchronous manager producing query builders."""

    def __init__(self, model_class: type[OxydeModel]) -> None:
        self.model_class = model_class

    def _query(self) -> Query:
        """Create a new Query for this model."""
        return Query(self.model_class)

    def filter(self, *args: Any, **kwargs: Any) -> Query:
        """
        Filter by Q-expressions or field lookups.

        Args:
            *args: Q expression objects for complex conditions (AND/OR/NOT)
            **kwargs: Field lookups (e.g., name__icontains="foo", age__gte=18)

        Returns:
            Query with filter conditions applied

        Examples:
            User.objects.filter(name="Alice")
            User.objects.filter(age__gte=18, status="active")
            User.objects.filter(Q(age__gte=18) | Q(premium=True))
        """
        return self._query().filter(*args, **kwargs)

    def query(self) -> Query:
        """Return a Query builder for this model.

        Use this to start building a query that will be executed later.
        For immediate execution, use async methods like all(), get(), etc.

        Examples:
            q = Post.objects.query().filter(status="active").order_by("-created")
            results = await q.all(using="default")
        """
        return self._query()

    def values(self, *fields: str) -> Query:
        return self._query().values(*fields)

    def values_list(self, *fields: str, flat: bool = False) -> Query:
        return self._query().values_list(*fields, flat=flat)

    def distinct(self, distinct: bool = True) -> Query:
        return self._query().distinct(distinct)

    def join(self, *paths: str) -> Query:
        return self._query().join(*paths)

    def prefetch(self, *paths: str) -> Query:
        return self._query().prefetch(*paths)

    def for_update(self) -> Query:
        return self._query().for_update()

    def for_share(self) -> Query:
        return self._query().for_share()

    def _primary_key_field(self) -> str | None:
        self.model_class.ensure_field_metadata()
        for field_name, meta in self.model_class._db_meta.field_metadata.items():
            if meta.primary_key:
                return field_name
        return None


class QueryManager(_QueryManagerBase):
    """Manager that executes queries using async connections only."""

    async def _resolve_client(
        self,
        client: SupportsExecute | None,
        using: str | None,
    ) -> SupportsExecute:
        """Resolve execution client. Delegates to shared _resolve_execution_client."""
        return await _resolve_execution_client(using, client)

    async def _execute_query(
        self,
        query: Query,
        client: SupportsExecute,
        mode: str,
    ) -> Any:
        if mode == "models":
            return await query.fetch_models(client)
        if mode == "dict":
            return await query.fetch_all(client)
        if mode == "msgpack":
            return await query.fetch_msgpack(client)
        raise ManagerError(f"Unsupported fetch mode '{mode}'")

    def _build_query(self, filters: dict[str, Any]) -> Query:
        query = self._query()
        if filters:
            return query.filter(**filters)
        return query

    def exclude(self, *args: Any, **kwargs: Any) -> Query:
        """
        Build query with negated conditions.

        Args:
            *args: Q expression objects to negate
            **kwargs: Field lookups to negate

        Returns:
            Query object

        Examples:
            User.objects.exclude(status="banned")
            User.objects.exclude(Q(age__lt=18) | Q(age__gt=65))
        """
        return self._query().exclude(*args, **kwargs)

    async def all(
        self,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
        mode: str = "models",
    ) -> Any:
        """Execute query and return all results.

        Args:
            client: Explicit client for execution
            using: Name of connection from registry
            mode: Result mode ("models", "dict", "list", "msgpack")

        Returns:
            list[Model], list[dict], etc. depending on mode
        """
        query = self._query()
        exec_client = await self._resolve_client(client, using)
        return await self._execute_query(query, exec_client, mode)

    async def get(
        self,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
        **filters: Any,
    ) -> Any:
        query = self._build_query(filters).limit(2)
        exec_client = await self._resolve_client(client, using)
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
        client: SupportsExecute | None = None,
        using: str | None = None,
        **filters: Any,
    ) -> Any | None:
        try:
            return await self.get(client=client, using=using, **filters)
        except NotFoundError:
            return None

    async def first(
        self,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
    ) -> Any | None:
        query = self._query()
        pk = self._primary_key_field()
        if pk:
            query = query.order_by(pk)
        query = query.limit(1)
        exec_client = await self._resolve_client(client, using)
        results = await query.fetch_models(exec_client)
        return results[0] if results else None

    async def last(
        self,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
    ) -> Any | None:
        query = self._query()
        pk = self._primary_key_field()
        if pk:
            query = query.order_by(f"-{pk}")
        query = query.limit(1)
        exec_client = await self._resolve_client(client, using)
        results = await query.fetch_models(exec_client)
        return results[0] if results else None

    async def count(
        self,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
    ) -> int:
        """Count all records using SQL COUNT(*)."""
        return await self._query().count(client=client, using=using)

    async def sum(
        self,
        field: str,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
    ):
        """Calculate sum of field values for all records."""
        return await self._query().sum(field, client=client, using=using)

    async def avg(
        self,
        field: str,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
    ):
        """Calculate average of field values for all records."""
        return await self._query().avg(field, client=client, using=using)

    async def max(
        self,
        field: str,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
    ):
        """Get maximum field value from all records."""
        return await self._query().max(field, client=client, using=using)

    async def min(
        self,
        field: str,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
    ):
        """Get minimum field value from all records."""
        return await self._query().min(field, client=client, using=using)

    async def _run_mutation(
        self, query: Any, client: SupportsExecute
    ) -> dict[str, Any]:
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
        client: SupportsExecute | None = None,
        using: str | None = None,
        _skip_hooks: bool = False,
        **data: Any,
    ) -> OxydeModel:
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

        exec_client = await self._resolve_client(client, using)
        payload = _dump_insert_data(instance)
        if not payload:
            raise ManagerError("create() requires at least one value")
        query = InsertQuery(self.model_class).values(**payload)
        result = await self._run_mutation(query, exec_client)

        # Assign auto-generated PK to instance
        if "inserted_ids" in result and result["inserted_ids"]:
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
        client: SupportsExecute | None = None,
        using: str | None = None,
        batch_size: int | None = None,
    ) -> list[OxydeModel]:
        """
        Insert multiple objects efficiently.

        Args:
            objects: Iterable of model instances or dicts
            client: Optional client for execution
            using: Optional database name
            batch_size: Optional batch size. If None, inserts all in one query.
                Use if hitting DB param limits (SQLite: 999, Postgres: 65535).

        Returns:
            List of created model instances
        """
        instances = [_normalize_instance(self.model_class, obj) for obj in objects]
        if not instances:
            return []

        exec_client = await self._resolve_client(client, using)

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
        client: SupportsExecute | None = None,
        using: str | None = None,
    ) -> int:
        """
        Bulk update multiple objects with CASE WHEN.

        Args:
            objects: Instances to update
            fields: Field names to update
            client: Optional database client
            using: Database alias

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

        exec_client = await self._resolve_client(client, using)

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

        # Use IR directly for bulk update
        from oxyde.core import ir
        from oxyde.queries.base import _build_col_types, _model_key

        col_types = _build_col_types(self.model_class)
        update_ir = ir.build_update_ir(
            table=self.model_class.get_table_name(),
            bulk_update=bulk_entries,
            col_types=col_types,
            model=_model_key(self.model_class),
        )

        import msgpack

        result_bytes = await exec_client.execute(update_ir)
        result = msgpack.unpackb(result_bytes, raw=False)
        return int(result.get("affected", 0))

    async def get_or_create(
        self,
        *,
        defaults: dict[str, Any] | None = None,
        client: SupportsExecute | None = None,
        using: str | None = None,
        **filters: Any,
    ) -> tuple[OxydeModel, bool]:
        try:
            obj = await self.get(client=client, using=using, **filters)
            return obj, False
        except NotFoundError:
            create_data = _derive_create_data(filters, defaults)
            obj = await self.create(client=client, using=using, **create_data)
            return obj, True

    async def upsert(
        self, *args: Any, **kwargs: Any
    ) -> Any:  # pragma: no cover - placeholder
        raise ManagerError("upsert() is not implemented yet")


__all__ = [
    "QueryManager",
]

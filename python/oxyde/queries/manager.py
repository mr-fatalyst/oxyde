"""Django-style QueryManager for Model.objects interface.

This module provides QueryManager - the class behind Model.objects that
enables Django-like query syntax. Each Model gets a QueryManager
instance automatically assigned to its `objects` attribute.

Design:
    QueryManager is a thin proxy that creates Query instances.
    All actual functionality lives in Query and its mixins.
    Manager methods just return Query(model) with the appropriate method called.

Example:
    # Manager is auto-attached to models
    class User(Model):
        ...

    # All methods return Query objects or execute queries
    users = await User.objects.all()
    user = await User.objects.filter(id=1).get()
    new_user = await User.objects.create(name="Alice")
"""

from __future__ import annotations

from collections.abc import Iterable
from typing import TYPE_CHECKING, Any, Literal, overload

from oxyde.exceptions import IntegrityError, ManagerError, NotFoundError
from oxyde.models.serializers import _derive_create_data, _dump_insert_data
from oxyde.queries.base import _resolve_execution_client
from oxyde.queries.insert import InsertQuery
from oxyde.queries.mixins.mutation import (
    _client_alias,
    _decode_returning_models,
    _is_mysql,
)
from oxyde.queries.select import Query

if TYPE_CHECKING:
    from oxyde.models.base import Model
    from oxyde.queries.base import SupportsExecute


class QueryManager:
    """Manager that provides Query access for a model.

    All methods delegate to Query - this is just a factory/proxy.
    """

    def __init__(self, model_class: type[Model]) -> None:
        self.model_class = model_class

    def _query(self) -> Query:
        """Create a new Query for this model."""
        return Query(self.model_class)

    # --- Query building methods (return Query) ---

    def query(self) -> Query:
        """Return a Query builder for this model."""
        return self._query()

    def filter(self, *args: Any, **kwargs: Any) -> Query:
        """Filter by Q-expressions or field lookups."""
        return self._query().filter(*args, **kwargs)

    def exclude(self, *args: Any, **kwargs: Any) -> Query:
        """Exclude rows matching conditions (NOT filter)."""
        return self._query().exclude(*args, **kwargs)

    def values(self, *fields: str) -> Query:
        """Return dicts instead of models."""
        return self._query().values(*fields)

    def values_list(self, *fields: str, flat: bool = False) -> Query:
        """Return tuples/lists instead of models."""
        return self._query().values_list(*fields, flat=flat)

    def distinct(self, distinct: bool = True) -> Query:
        """Add DISTINCT to query."""
        return self._query().distinct(distinct)

    def join(self, *paths: str) -> Query:
        """Eager load relations via JOIN."""
        return self._query().join(*paths)

    def prefetch(self, *paths: str) -> Query:
        """Prefetch relations in separate queries."""
        return self._query().prefetch(*paths)

    def for_update(self) -> Query:
        """Add FOR UPDATE lock."""
        return self._query().for_update()

    def for_share(self) -> Query:
        """Add FOR SHARE lock."""
        return self._query().for_share()

    def order_by(self, *fields: str) -> Query:
        """Order results by fields. Prefix with '-' for descending."""
        return self._query().order_by(*fields)

    def limit(self, value: int) -> Query:
        """Limit number of results."""
        return self._query().limit(value)

    def offset(self, value: int) -> Query:
        """Skip first N results."""
        return self._query().offset(value)

    def annotate(self, **annotations: Any) -> Query:
        """Add aggregate annotations."""
        return self._query().annotate(**annotations)

    # --- Execution methods (delegate to Query) ---

    def all(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
        mode: str = "models",
    ):
        """Execute query and return all results.

        Args:
            using: Database alias
            client: Optional database client
            mode: Result mode ("models", "dict", "list", "msgpack")
        """
        q = self._query()
        if mode == "dict":
            q = q.values()
        elif mode == "list":
            q = q.values_list()
        elif mode == "msgpack":
            q._result_mode = "msgpack"
        return q.all(using=using, client=client)

    async def first(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Return first result or None."""
        return await self._query().first(using=using, client=client)

    async def last(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Return last result or None."""
        return await self._query().last(using=using, client=client)

    async def get(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
        **filters: Any,
    ):
        """Return exactly one result. Raises NotFoundError/MultipleObjectsReturned."""
        q = self._query()
        if filters:
            q = q.filter(**filters)
        return await q.get(using=using, client=client)

    async def get_or_none(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
        **filters: Any,
    ):
        """Return one result or None. Raises MultipleObjectsReturned if many."""
        q = self._query()
        if filters:
            q = q.filter(**filters)
        return await q.get_or_none(using=using, client=client)

    async def get_or_create(
        self,
        *,
        defaults: dict[str, Any] | None = None,
        using: str | None = None,
        client: SupportsExecute | None = None,
        **filters: Any,
    ) -> tuple[Model, bool]:
        """Get existing object or create a new one.

        Args:
            defaults: Field values to use when creating (not for lookup)
            using: Database alias
            client: Optional database client
            **filters: Lookup conditions for finding existing object

        Returns:
            Tuple of (instance, created) where created is True if new object was made
        """
        try:
            obj = await self.get(using=using, client=client, **filters)
            return obj, False
        except NotFoundError:
            create_data = _derive_create_data(filters, defaults)
            try:
                obj = await self.create(using=using, client=client, **create_data)
                return obj, True
            except IntegrityError:
                obj = await self.get(using=using, client=client, **filters)
                return obj, False

    async def exists(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> bool:
        """Check if any records match."""
        return await self._query().exists(using=using, client=client)

    async def count(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> int:
        """Count all records using SQL COUNT(*)."""
        return await self._query().count(using=using, client=client)

    async def sum(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Calculate sum of field values."""
        return await self._query().sum(field, using=using, client=client)

    async def avg(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Calculate average of field values."""
        return await self._query().avg(field, using=using, client=client)

    async def max(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Get maximum field value."""
        return await self._query().max(field, using=using, client=client)

    async def min(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ):
        """Get minimum field value."""
        return await self._query().min(field, using=using, client=client)

    # --- Mutation methods (delegate to Query) ---

    async def create(
        self,
        *,
        instance: Model | None = None,
        using: str | None = None,
        client: SupportsExecute | None = None,
        _skip_hooks: bool = False,
        **data: Any,
    ) -> Model:
        """Create a new record in the database."""
        return await self._query().create(
            instance=instance,
            using=using,
            client=client,
            _skip_hooks=_skip_hooks,
            **data,
        )

    async def bulk_create(
        self,
        objects: Iterable[Any],
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
        batch_size: int | None = None,
    ) -> list[Model]:
        """Bulk insert multiple objects efficiently."""
        return await self._query().bulk_create(
            objects,
            using=using,
            client=client,
            batch_size=batch_size,
        )

    async def bulk_update(
        self,
        objects: Iterable[Model],
        fields: Iterable[str],
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> int:
        """Bulk update multiple objects with CASE WHEN."""
        return await self._query().bulk_update(
            objects,
            fields,
            using=using,
            client=client,
        )

    async def update_or_create(
        self,
        *,
        defaults: dict[str, Any] | None = None,
        using: str | None = None,
        client: SupportsExecute | None = None,
        **filters: Any,
    ) -> tuple[Model, bool]:
        """Get existing object and update it, or create it if it does not exist.

        Args:
            defaults: Field values to use when creating or updating
            using: Database alias
            client: Optional database client
            **filters: Lookup conditions for finding existing object

        Returns:
            Tuple of (instance, created) where created is True if a new
            object was made.
        """
        try:
            obj = await self.get(using=using, client=client, **filters)
        except NotFoundError:
            create_data = _derive_create_data(filters, defaults)
            try:
                obj = await self.create(using=using, client=client, **create_data)
                return obj, True
            except IntegrityError:
                obj = await self.get(using=using, client=client, **filters)

        if not defaults:
            return obj, False

        for key, value in defaults.items():
            setattr(obj, key, value)

        saved_obj = await obj.save(
            client=client,
            using=using,
            update_fields=defaults.keys(),
        )
        return saved_obj, False

    @overload
    async def upsert(
        self,
        *,
        defaults: dict[str, Any],
        returning: Literal[True],
        using: str | None = ...,
        client: SupportsExecute | None = ...,
        **conflict_values: Any,
    ) -> list[Model]: ...

    @overload
    async def upsert(
        self,
        *,
        defaults: dict[str, Any],
        returning: Literal[False] = ...,
        using: str | None = ...,
        client: SupportsExecute | None = ...,
        **conflict_values: Any,
    ) -> int: ...

    async def upsert(
        self,
        *,
        defaults: dict[str, Any],
        returning: bool = False,
        using: str | None = None,
        client: SupportsExecute | None = None,
        **conflict_values: Any,
    ) -> int | list[Model]:
        """Execute a backend-native upsert keyed by exact model field kwargs.

        Args:
            defaults: Values to insert and update when the keyed row already
                exists. Must be non-empty, must not overlap the key fields,
                and must include any non-key fields required for inserts.
            returning: If True, return inserted/updated model instances.
                Default False returns affected row count.
            using: Database alias
            client: Optional database client
            **conflict_values: Exact model field values identifying the unique
                row to upsert. These fields must map to a primary key or unique
                constraint in the database.

        Returns:
            Number of affected rows by default, or list of inserted/updated
            model instances if returning=True. Uses native SQL conflict
            handling and does not run save() hooks.
        """
        conflict_fields = list(conflict_values)
        if not conflict_fields:
            raise ValueError("upsert requires at least one conflict field")
        if not defaults:
            raise ValueError("upsert requires non-empty defaults")

        lookup_fields = [field for field in conflict_fields if "__" in field]
        if lookup_fields:
            lookup_list = ", ".join(sorted(lookup_fields))
            raise ValueError(
                f"upsert conflict fields must be exact model field names: {lookup_list}"
            )

        default_lookup_fields = [field for field in defaults if "__" in field]
        if default_lookup_fields:
            lookup_list = ", ".join(sorted(default_lookup_fields))
            raise ValueError(
                f"upsert defaults must be exact model field names: {lookup_list}"
            )

        overlapping = sorted(set(conflict_fields) & set(defaults))
        if overlapping:
            overlap = ", ".join(overlapping)
            raise ValueError(
                f"upsert defaults cannot include conflict fields: {overlap}"
            )

        values = {**conflict_values, **defaults}
        instance = self.model_class(**values)
        insert_values = _dump_insert_data(instance)
        if not insert_values:
            raise ValueError("upsert requires at least one insertable value")

        missing_insertables = [
            field for field in conflict_fields if field not in insert_values
        ]
        if missing_insertables:
            missing = ", ".join(sorted(missing_insertables))
            raise ValueError(
                f"upsert conflict fields must map to insertable model fields: {missing}"
            )

        missing_default_insertables = [
            field for field in defaults if field not in insert_values
        ]
        if missing_default_insertables:
            missing = ", ".join(sorted(missing_default_insertables))
            raise ValueError(
                f"upsert defaults must map to insertable model fields: {missing}"
            )

        resolved_update_values = {
            key: insert_values[key] for key in defaults if key in insert_values
        }
        if not resolved_update_values:
            raise ValueError("upsert requires at least one insertable default value")

        exec_client = await _resolve_execution_client(using, client)
        query = (
            InsertQuery(self.model_class)
            .values(**insert_values)
            .returning(returning)
            .on_conflict(
                columns=conflict_fields,
                action="update",
                update_values=resolved_update_values,
            )
        )
        if returning and await _is_mysql(using, client):
            return await self._mysql_upsert_returning(
                query=query,
                conflict_values=conflict_values,
                insert_values=insert_values,
                using=using,
                client=client,
                exec_client=exec_client,
            )

        result = await self._query()._run_mutation(query, exec_client)
        if returning:
            return _decode_returning_models(self.model_class, result)
        return int(result.get("affected", 0))

    async def _mysql_upsert_returning(
        self,
        *,
        query: InsertQuery,
        conflict_values: dict[str, Any],
        insert_values: dict[str, Any],
        using: str | None,
        client: SupportsExecute | None,
        exec_client: SupportsExecute,
    ) -> list[Model]:
        """MySQL fallback for upsert(returning=True): upsert, then re-select."""
        from oxyde.db.pool import AsyncDatabase
        from oxyde.db.transaction import (
            AsyncTransaction,
            AtomicTransactionContext,
            atomic,
            get_active_transaction,
        )

        alias = _client_alias(using, client)

        async def _do(tx_client: SupportsExecute) -> list[Model]:
            result = await self._query()._run_mutation(
                query.returning(False), tx_client
            )
            refetch_filters = self._mysql_upsert_refetch_filters(
                result=result,
                conflict_values=conflict_values,
                insert_values=insert_values,
            )
            return await self.filter(**refetch_filters).all(client=tx_client)

        if isinstance(client, AsyncTransaction):
            return await _do(exec_client)

        if get_active_transaction(alias) is not None:
            return await _do(exec_client)

        if isinstance(client, AsyncDatabase):
            async with AtomicTransactionContext(using=alias, database=client) as tx:
                return await _do(tx)

        if client is not None:
            # Custom execute clients may be stubs or wrappers; reuse them as-is.
            return await _do(exec_client)

        async with atomic(using=alias) as tx:
            return await _do(tx)

    def _mysql_upsert_refetch_filters(
        self,
        *,
        result: dict[str, Any],
        conflict_values: dict[str, Any],
        insert_values: dict[str, Any],
    ) -> dict[str, Any]:
        """Choose a safe re-select filter for MySQL upsert(returning=True)."""
        affected = int(result.get("affected", 0))
        pk_field = self.model_class._db_meta.pk_field

        if affected == 1 and pk_field:
            inserted_ids = result.get("inserted_ids", [])
            if inserted_ids:
                return {pk_field: inserted_ids[0]}

            pk_value = insert_values.get(pk_field)
            if pk_value is not None:
                return {pk_field: pk_value}

        nullable_conflicts = sorted(
            field for field, value in conflict_values.items() if value is None
        )
        if affected == 1 and nullable_conflicts:
            fields = ", ".join(nullable_conflicts)
            raise ManagerError(
                "MySQL upsert(returning=True) cannot safely refetch inserted rows "
                f"with NULL conflict fields without a primary key: {fields}"
            )

        return conflict_values


__all__ = [
    "QueryManager",
]

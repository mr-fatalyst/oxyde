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

from collections.abc import Coroutine, Iterable
from typing import TYPE_CHECKING, Any, Generic, Literal, TypeVar, overload

from oxyde.exceptions import IntegrityError, ManagerError, NotFoundError
from oxyde.models.serializers import _derive_create_data
from oxyde.queries.select import Query

if TYPE_CHECKING:
    from oxyde.models.base import Model
    from oxyde.queries.base import SupportsExecute


TModel = TypeVar("TModel", bound="Model")


class QueryManager(Generic[TModel]):
    """Manager that provides Query access for a model.

    All methods delegate to Query - this is just a factory/proxy.
    """

    def __init__(self, model_class: type[TModel]) -> None:
        self.model_class = model_class

    def _query(self) -> Query[TModel]:
        """Create a new Query for this model."""
        return Query(self.model_class)

    # --- Query building methods (return Query) ---

    def query(self) -> Query[TModel]:
        """Return a Query builder for this model."""
        return self._query()

    def filter(self, *args: Any, **kwargs: Any) -> Query[TModel]:
        """Filter by Q-expressions or field lookups."""
        return self._query().filter(*args, **kwargs)

    def exclude(self, *args: Any, **kwargs: Any) -> Query[TModel]:
        """Exclude rows matching conditions (NOT filter)."""
        return self._query().exclude(*args, **kwargs)

    def values(self, *fields: str) -> Query[TModel]:
        """Return dicts instead of models."""
        return self._query().values(*fields)

    def values_list(self, *fields: str, flat: bool = False) -> Query[TModel]:
        """Return tuples/lists instead of models."""
        return self._query().values_list(*fields, flat=flat)

    def distinct(self, distinct: bool = True) -> Query[TModel]:
        """Add DISTINCT to query."""
        return self._query().distinct(distinct)

    def join(self, *paths: str) -> Query[TModel]:
        """Eager load relations via JOIN."""
        return self._query().join(*paths)

    def prefetch(self, *paths: str) -> Query[TModel]:
        """Prefetch relations in separate queries."""
        return self._query().prefetch(*paths)

    def for_update(self) -> Query[TModel]:
        """Add FOR UPDATE lock."""
        return self._query().for_update()

    def for_share(self) -> Query[TModel]:
        """Add FOR SHARE lock."""
        return self._query().for_share()

    def order_by(self, *fields: str) -> Query[TModel]:
        """Order results by fields. Prefix with '-' for descending."""
        return self._query().order_by(*fields)

    def limit(self, value: int) -> Query[TModel]:
        """Limit number of results."""
        return self._query().limit(value)

    def offset(self, value: int) -> Query[TModel]:
        """Skip first N results."""
        return self._query().offset(value)

    def annotate(self, **annotations: Any) -> Query[TModel]:
        """Add aggregate annotations."""
        return self._query().annotate(**annotations)

    # --- Execution methods (delegate to Query) ---

    @overload
    def all(
        self,
        *,
        using: str | None = ...,
        client: SupportsExecute | None = ...,
        mode: Literal["models"],
    ) -> Coroutine[Any, Any, list[TModel]]: ...

    @overload
    def all(
        self,
        *,
        using: str | None = ...,
        client: SupportsExecute | None = ...,
    ) -> Coroutine[Any, Any, list[TModel]]: ...

    @overload
    def all(
        self,
        *,
        using: str | None = ...,
        client: SupportsExecute | None = ...,
        mode: Literal["msgpack"],
    ) -> Coroutine[Any, Any, bytes]: ...

    @overload
    def all(
        self,
        *,
        using: str | None = ...,
        client: SupportsExecute | None = ...,
        mode: Literal["dict"],
    ) -> Coroutine[Any, Any, list[dict[str, Any]]]: ...

    @overload
    def all(
        self,
        *,
        using: str | None = ...,
        client: SupportsExecute | None = ...,
        mode: Literal["list"],
    ) -> Coroutine[Any, Any, list[tuple[Any, ...]]]: ...

    @overload
    def all(
        self,
        *,
        using: str | None = ...,
        client: SupportsExecute | None = ...,
        mode: str,
    ) -> Coroutine[Any, Any, bytes | list[Any]]: ...

    def all(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
        mode: str = "models",
    ) -> Coroutine[Any, Any, bytes | list[Any]]:
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
    ) -> TModel | None:
        """Return first result or None."""
        return await self._query().first(using=using, client=client)

    async def last(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> TModel | None:
        """Return last result or None."""
        return await self._query().last(using=using, client=client)

    async def get(
        self,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
        **filters: Any,
    ) -> TModel:
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
    ) -> TModel | None:
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
    ) -> tuple[TModel, bool]:
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
    ) -> Any:
        """Calculate sum of field values."""
        return await self._query().sum(field, using=using, client=client)

    async def avg(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> Any:
        """Calculate average of field values."""
        return await self._query().avg(field, using=using, client=client)

    async def max(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> Any:
        """Get maximum field value."""
        return await self._query().max(field, using=using, client=client)

    async def min(
        self,
        field: str,
        *,
        using: str | None = None,
        client: SupportsExecute | None = None,
    ) -> Any:
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
    ) -> TModel:
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
    ) -> list[TModel]:
        """Bulk insert multiple objects efficiently."""
        return await self._query().bulk_create(
            objects,
            using=using,
            client=client,
            batch_size=batch_size,
        )

    async def bulk_update(
        self,
        objects: Iterable[TModel],
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
    ) -> tuple[TModel, bool]:
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

    async def upsert(self, *args: Any, **kwargs: Any) -> Any:
        """Not implemented yet."""
        raise ManagerError("upsert() is not implemented yet")


__all__ = [
    "QueryManager",
]

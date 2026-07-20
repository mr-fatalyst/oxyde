"""Typing-only Query subclasses used by generated .pyi stubs.

At runtime ``values()`` / ``values_list()`` return the same ``Query`` object
with a result-mode flag flipped; generated stubs re-type those calls with the
classes below so terminal methods reflect what actually comes back (dicts,
tuples or scalars instead of model instances). Mode-switching methods are
re-declared so chains like ``values().values_list()`` keep tracking the
actual result shape. The classes carry no behaviour of their own and are
never instantiated — if one ever were, it would behave exactly like a plain
``Query``.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeVar

from oxyde.models.base import Model
from oxyde.queries.select import Query

if TYPE_CHECKING:
    from collections.abc import Coroutine
    from typing import Literal, overload

    from oxyde.queries.base import SupportsExecute

TModel = TypeVar("TModel", bound=Model)


class ValuesQuery(Query[TModel]):
    """Query in ``.values()`` mode — rows come back as dicts."""

    if TYPE_CHECKING:

        @overload  # type: ignore[override]
        def values_list(
            self, field: str, /, *, flat: Literal[True]
        ) -> FlatValuesListQuery[TModel]: ...

        @overload
        def values_list(
            self, *fields: str, flat: Literal[False] = ...
        ) -> ValuesListQuery[TModel]: ...

        @overload
        def values_list(
            self, *fields: str, flat: bool
        ) -> ValuesListQuery[TModel] | FlatValuesListQuery[TModel]: ...

        def values_list(self, *fields: str, flat: bool = False) -> Any: ...

        def all(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> Coroutine[Any, Any, list[dict[str, Any]]]: ...

        async def first(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> dict[str, Any] | None: ...

        async def last(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> dict[str, Any] | None: ...

        async def get(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> dict[str, Any]: ...

        async def get_or_none(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> dict[str, Any] | None: ...


class ValuesListQuery(Query[TModel]):
    """Query in ``.values_list()`` mode — rows come back as tuples."""

    if TYPE_CHECKING:

        def values(  # type: ignore[override]
            self, *fields: str
        ) -> ValuesQuery[TModel]: ...

        @overload  # type: ignore[override]
        def values_list(
            self, field: str, /, *, flat: Literal[True]
        ) -> FlatValuesListQuery[TModel]: ...

        @overload
        def values_list(
            self, *fields: str, flat: Literal[False] = ...
        ) -> ValuesListQuery[TModel]: ...

        @overload
        def values_list(
            self, *fields: str, flat: bool
        ) -> ValuesListQuery[TModel] | FlatValuesListQuery[TModel]: ...

        def values_list(self, *fields: str, flat: bool = False) -> Any: ...

        def all(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> Coroutine[Any, Any, list[tuple[Any, ...]]]: ...

        async def first(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> tuple[Any, ...] | None: ...

        async def last(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> tuple[Any, ...] | None: ...

        async def get(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> tuple[Any, ...]: ...

        async def get_or_none(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> tuple[Any, ...] | None: ...


class FlatValuesListQuery(Query[TModel]):
    """Query in ``.values_list(..., flat=True)`` mode — rows are scalars."""

    if TYPE_CHECKING:

        def values(  # type: ignore[override]
            self, *fields: str
        ) -> ValuesQuery[TModel]: ...

        @overload  # type: ignore[override]
        def values_list(
            self, field: str, /, *, flat: Literal[True]
        ) -> FlatValuesListQuery[TModel]: ...

        @overload
        def values_list(
            self, *fields: str, flat: Literal[False] = ...
        ) -> ValuesListQuery[TModel]: ...

        @overload
        def values_list(
            self, *fields: str, flat: bool
        ) -> ValuesListQuery[TModel] | FlatValuesListQuery[TModel]: ...

        def values_list(self, *fields: str, flat: bool = False) -> Any: ...

        def all(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> Coroutine[Any, Any, list[Any]]: ...

        async def first(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> Any: ...

        async def last(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> Any: ...

        async def get(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> Any: ...

        async def get_or_none(
            self,
            *,
            using: str | None = None,
            client: SupportsExecute | None = None,
        ) -> Any: ...


__all__ = [
    "FlatValuesListQuery",
    "ValuesListQuery",
    "ValuesQuery",
]

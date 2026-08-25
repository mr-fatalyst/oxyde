"""Pagination mixin for query building."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Generic, TypeVar

from typing_extensions import Self

if TYPE_CHECKING:
    from typing import Literal, overload

    from oxyde.models.base import Model
    from oxyde.queries.typed import (
        FlatValuesListQuery,
        ValuesListQuery,
        ValuesQuery,
    )


TModel = TypeVar("TModel", bound="Model")


class PaginationMixin(Generic[TModel]):
    """Mixin providing pagination and ordering capabilities."""

    # These attributes are defined in the base Query class
    _limit_value: int | None
    _offset_value: int | None
    _order_by_fields: list[tuple[str, str]]
    _distinct: bool
    _result_mode: str | None
    _values_flat: bool
    _selected_fields: list[str] | None

    def _clone(self) -> Self:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def select(self, *fields: str) -> Self:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def limit(self, value: int) -> Self:
        """Set LIMIT."""
        if value < 0:
            raise ValueError(f"limit() requires a non-negative value, got {value}")
        clone = self._clone()
        clone._limit_value = value
        return clone

    def offset(self, value: int) -> Self:
        """Set OFFSET."""
        if value < 0:
            raise ValueError(f"offset() requires a non-negative value, got {value}")
        clone = self._clone()
        clone._offset_value = value
        return clone

    def order_by(self, *fields: str) -> Self:
        """Set ORDER BY fields. Use "?" for random ordering (ORDER BY RANDOM())."""
        clone = self._clone()
        for field in fields:
            if field == "?":
                clone._order_by_fields.append(("?", "RANDOM"))
            elif field.startswith("-"):
                clone._order_by_fields.append((field[1:], "DESC"))
            else:
                clone._order_by_fields.append((field, "ASC"))
        return clone

    def distinct(self, distinct: bool = True) -> Self:
        """Set DISTINCT."""
        clone = self._clone()
        clone._distinct = bool(distinct)
        return clone

    # values()/values_list() flip a result-mode flag on a clone at runtime;
    # statically they are declared to return the typing facades from
    # oxyde.queries.typed so terminal methods reflect the actual row shape
    # (dicts / tuples / scalars) instead of model instances.
    if TYPE_CHECKING:

        def values(self, *fields: str) -> ValuesQuery[TModel]:
            """Return results as dictionaries."""
            ...

        @overload
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

        def values_list(self, *fields: str, flat: bool = False) -> Any:
            """Return results as tuples (or flat list if flat=True)."""
            ...

    else:

        def values(self, *fields):
            """Return results as dictionaries."""
            clone = self._clone()
            if fields:
                clone = clone.select(*fields)
            clone._result_mode = "dict"
            return clone

        def values_list(self, *fields, flat=False):
            """Return results as tuples (or flat list if flat=True)."""
            clone = self._clone()
            if fields:
                clone = clone.select(*fields)
            if flat and fields and len(fields) != 1:
                raise ValueError(
                    "flat=True is only valid when a single field is selected"
                )
            clone._result_mode = "list"
            clone._values_flat = flat
            return clone

    def __getitem__(self, key: slice | int) -> Self:
        """Support slicing: query[0:10] or query[5]."""
        if isinstance(key, slice):
            start = key.start or 0
            stop = key.stop
            if stop is None:
                raise ValueError("Slicing a Query requires an end index")
            if start < 0 or (stop is not None and stop < 0):
                raise ValueError("Negative slicing is not supported")
            clone = self.offset(start)
            length = max(0, stop - start)
            return clone.limit(length)
        if isinstance(key, int):
            if key < 0:
                raise ValueError("Negative indexing is not supported")
            return self.offset(key).limit(1)
        raise TypeError("Invalid argument type for slicing Query")

"""Pagination mixin for query building."""

from __future__ import annotations

from oxyde.queries.base import TQuery


class PaginationMixin:
    """Mixin providing pagination and ordering capabilities."""

    # These attributes are defined in the base Query class
    _limit_value: int | None
    _offset_value: int | None
    _order_by_fields: list[tuple[str, str]]
    _distinct: bool
    _result_mode: str | None
    _values_flat: bool
    _selected_fields: list[str] | None

    def _clone(self: TQuery) -> TQuery:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def select(self, *fields: str) -> TQuery:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def limit(self: TQuery, value: int) -> TQuery:
        """Set LIMIT."""
        clone = self._clone()
        clone._limit_value = value
        return clone

    def offset(self: TQuery, value: int) -> TQuery:
        """Set OFFSET."""
        clone = self._clone()
        clone._offset_value = value
        return clone

    def order_by(self: TQuery, *fields: str) -> TQuery:
        """Set ORDER BY fields."""
        clone = self._clone()
        for field in fields:
            if field.startswith("-"):
                clone._order_by_fields.append((field[1:], "DESC"))
            else:
                clone._order_by_fields.append((field, "ASC"))
        return clone

    def distinct(self: TQuery, distinct: bool = True) -> TQuery:
        """Set DISTINCT."""
        clone = self._clone()
        clone._distinct = bool(distinct)
        return clone

    def values(self: TQuery, *fields: str) -> TQuery:
        """Return results as dictionaries."""
        clone = self._clone()
        if fields:
            clone = clone.select(*fields)
        clone._result_mode = "dict"
        return clone

    def values_list(self: TQuery, *fields: str, flat: bool = False) -> TQuery:
        """Return results as tuples (or flat list if flat=True and single field)."""
        clone = self._clone()
        if fields:
            clone = clone.select(*fields)
        if flat and fields and len(fields) != 1:
            raise ValueError("flat=True is only valid when a single field is selected")
        clone._result_mode = "list"
        clone._values_flat = flat
        return clone

    def __getitem__(self: TQuery, key: slice | int) -> TQuery:
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

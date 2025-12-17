"""Filtering mixin for query building."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from oxyde.core import ir
from oxyde.queries.base import TQuery

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


class FilteringMixin:
    """Mixin providing filtering capabilities."""

    # These attributes are defined in the base Query class
    _filter_tree: ir.FilterNode | None
    model_class: type[OxydeModel]

    def _clone(self: TQuery) -> TQuery:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def filter(self: TQuery, *args: Any, **kwargs: Any) -> TQuery:
        """
        Filter by Q-expressions or field lookups.

        Supports filtering on related model fields via FK traversal:
            .filter(user__age__gte=18)  # Filter by joined user.age

        Args:
            *args: Q expression objects for complex conditions (AND/OR/NOT)
            **kwargs: Field lookups (e.g., name__icontains="foo", age__gte=18,
                      user__name="Alice")

        Returns:
            Query with filter conditions applied

        Examples:
            .filter(name="Alice")
            .filter(age__gte=18, status="active")
            .filter(Q(age__gte=18) | Q(premium=True))
            .filter(Q(name="Bob"), age__lt=30)
            .filter(user__age__gte=18)  # FK traversal
        """
        from oxyde.queries.q import Q

        clone = self._clone()
        conditions: list[ir.FilterNode] = []

        # Process Q-expressions from args
        for arg in args:
            if isinstance(arg, Q):
                node = arg.to_filter_node(self.model_class, clone)
                if node:
                    conditions.append(node)
            else:
                raise TypeError(
                    f"filter() positional args must be Q objects, got {type(arg)}"
                )

        # Process kwargs
        if kwargs:
            q_from_kwargs = Q(**kwargs)
            node = q_from_kwargs.to_filter_node(self.model_class, clone)
            if node:
                conditions.append(node)

        if not conditions:
            return clone

        # Merge with existing filter_tree
        if clone._filter_tree:
            conditions.insert(0, clone._filter_tree)

        if len(conditions) == 1:
            clone._filter_tree = conditions[0]
        else:
            clone._filter_tree = ir.filter_and(*conditions)

        return clone

    def exclude(self: TQuery, *args: Any, **kwargs: Any) -> TQuery:
        """
        Add negated filter conditions.

        Args:
            *args: Q expression objects to negate
            **kwargs: Field lookups to negate

        Returns:
            Cloned query with negated filter

        Examples:
            .exclude(status="banned")
            .exclude(Q(age__lt=18) | Q(age__gt=65))
            .exclude(user__age__lt=18)  # FK traversal
        """
        from oxyde.queries.q import Q

        clone = self._clone()
        conditions_to_negate: list[ir.FilterNode] = []

        # Process Q-expressions from args
        for arg in args:
            if isinstance(arg, Q):
                node = arg.to_filter_node(self.model_class, clone)
                if node:
                    conditions_to_negate.append(node)
            else:
                raise TypeError(
                    f"exclude() positional args must be Q objects, got {type(arg)}"
                )

        # Process kwargs
        if kwargs:
            q_from_kwargs = Q(**kwargs)
            node = q_from_kwargs.to_filter_node(self.model_class, clone)
            if node:
                conditions_to_negate.append(node)

        if not conditions_to_negate:
            return clone

        # Combine conditions with AND, then negate
        if len(conditions_to_negate) == 1:
            negated = ir.filter_not(conditions_to_negate[0])
        else:
            combined = ir.filter_and(*conditions_to_negate)
            negated = ir.filter_not(combined)

        # Merge negated filter into clone
        if clone._filter_tree:
            clone._filter_tree = ir.filter_and(clone._filter_tree, negated)
        else:
            clone._filter_tree = negated

        return clone

    def _build_filter_tree(self) -> ir.FilterNode | None:
        """Return the filter tree."""
        return self._filter_tree

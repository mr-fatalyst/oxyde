"""Q-expressions for complex boolean filter logic (AND/OR/NOT).

Q objects allow building complex WHERE clauses that cannot be expressed
with simple keyword arguments. They support boolean operators &, |, ~.

Basic Usage:
    Q(age__gte=18)          → age >= 18
    Q(name="Alice")         → name = 'Alice'
    Q(status__in=["a","b"]) → status IN ('a', 'b')

FK Traversal:
    Q(user__age__gte=18)    → JOIN user, WHERE user.age >= 18
    Q(author__name="Bob")   → JOIN author, WHERE author.name = 'Bob'

Combining with AND:
    Q(age__gte=18) & Q(status="active")
    → age >= 18 AND status = 'active'

Combining with OR:
    Q(role="admin") | Q(role="moderator")
    → role = 'admin' OR role = 'moderator'

Negation with NOT:
    ~Q(status="banned")
    → NOT (status = 'banned')

Complex Nesting:
    Q(age__gte=18) & (Q(status="active") | Q(premium=True))
    → age >= 18 AND (status = 'active' OR premium = true)

Implementation:
    Q stores either:
    - _kwargs: dict of field lookups (leaf node)
    - _op + _children: boolean operation with child Q objects

    to_filter_node(model_class, query) recursively converts Q tree to FilterNode
    IR format that Rust understands. If query is provided and FK paths are used,
    JOINs are automatically added to the query.

Usage in Queries:
    User.objects.filter(Q(age__gte=18) | Q(premium=True))
    User.objects.filter(Q(status="active"), name__startswith="A")  # mixed
    User.objects.exclude(Q(role="bot") | Q(status="banned"))
    Post.objects.filter(user__age__gte=18)  # FK traversal with auto-JOIN
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from oxyde.core.ir import FilterNode, filter_and, filter_not, filter_or

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


class Q:
    """
    Q-expression for building complex filter conditions with AND/OR/NOT logic.

    Usage:
        Q(age__gte=18) & Q(status="active")
        Q(age__gte=18) | Q(premium=True)
        ~Q(status="banned")
        Q(age__gte=18) & (Q(status="active") | Q(status="premium"))
    """

    def __init__(self, *args: Any, **kwargs: Any):
        """
        Create a Q expression from conditions.

        Args:
            **kwargs: Field lookups (e.g., age__gte=18, name__contains="John")
        """
        if args and not kwargs:
            # Q(condition_node) - wrap existing FilterNode
            if len(args) == 1 and isinstance(args[0], dict):
                self._node = args[0]
                self._kwargs = {}
            else:
                raise ValueError(
                    "Q() with positional args must receive a single FilterNode dict"
                )
        elif not args and kwargs:
            # Q(age=18, status="active") - build from kwargs
            self._node = None
            self._kwargs = kwargs
        elif not args and not kwargs:
            # Q() - empty, will be filtered out
            self._node = None
            self._kwargs = {}
        else:
            raise ValueError(
                "Q() accepts either positional FilterNode or keyword lookups, not both"
            )

    def _ensure_node(
        self,
        model_class: type[OxydeModel] | None = None,
        query: Any = None,
    ) -> FilterNode | None:
        """Convert kwargs to FilterNode if needed.

        Args:
            model_class: Model class for field resolution
            query: Optional query object for adding JOINs when FK paths are used
        """
        if self._node is not None:
            return self._node
        if not self._kwargs:
            return None

        # Import here to avoid circular dependency
        from oxyde.exceptions import FieldLookupError
        from oxyde.models.lookups import (
            _allowed_lookups_for_meta,
            _build_lookup_conditions,
            _parse_lookup_path,
            _resolve_field_path,
        )

        if model_class is None:
            raise ValueError(
                "Q expression with kwargs requires model_class to resolve fields"
            )

        model_class.ensure_field_metadata()
        conditions: list[FilterNode] = []

        for key, value in self._kwargs.items():
            field_path, lookup = _parse_lookup_path(key)

            # Single field (no FK traversal)
            if len(field_path) == 1:
                from oxyde.models.lookups import _resolve_column_meta

                field_name = field_path[0]
                column_meta = _resolve_column_meta(model_class, field_name)

                if lookup not in _allowed_lookups_for_meta(column_meta):
                    raise FieldLookupError(
                        f"Lookup '{lookup}' is not supported for field '{field_name}'"
                    )

                field_conditions = _build_lookup_conditions(
                    model_class,
                    field_name,
                    lookup,
                    value,
                    column_meta,
                )
                conditions.extend([c.to_ir() for c in field_conditions])
            else:
                # FK traversal (e.g., user__age__gte)
                resolved = _resolve_field_path(model_class, field_path)

                if lookup not in _allowed_lookups_for_meta(resolved.column_meta):
                    raise FieldLookupError(
                        f"Lookup '{lookup}' is not supported for field '{resolved.final_field}'"
                    )

                # Add JOINs to query if provided
                if query is not None and resolved.joins:
                    join_path = "__".join(name for name, _ in resolved.joins)
                    query._add_join_path(join_path)

                # Build qualified column name: "alias.column"
                # The alias is the join path with __ replaced by _
                alias = "_".join(name for name, _ in resolved.joins)
                qualified_column = f"{alias}.{resolved.column_meta.db_column}"

                field_conditions = _build_lookup_conditions(
                    resolved.final_model,
                    resolved.final_field,
                    lookup,
                    value,
                    resolved.column_meta,
                )
                # Override column with qualified name
                for cond in field_conditions:
                    cond.column = qualified_column
                conditions.extend([c.to_ir() for c in field_conditions])

        if not conditions:
            return None
        if len(conditions) == 1:
            return conditions[0]
        return filter_and(*conditions)

    def __and__(self, other: Q) -> Q:
        """Combine with AND logic: Q(...) & Q(...)"""
        if not isinstance(other, Q):
            raise TypeError(f"Cannot AND Q with {type(other)}")
        result = Q()
        result._node = None  # Will be computed lazily
        result._kwargs = {}
        result._op = "and"
        result._children = [self, other]
        return result

    def __or__(self, other: Q) -> Q:
        """Combine with OR logic: Q(...) | Q(...)"""
        if not isinstance(other, Q):
            raise TypeError(f"Cannot OR Q with {type(other)}")
        result = Q()
        result._node = None
        result._kwargs = {}
        result._op = "or"
        result._children = [self, other]
        return result

    def __invert__(self) -> Q:
        """Negate with NOT logic: ~Q(...)"""
        result = Q()
        result._node = None
        result._kwargs = {}
        result._op = "not"
        result._children = [self]
        return result

    def to_filter_node(
        self,
        model_class: type[OxydeModel],
        query: Any = None,
    ) -> FilterNode | None:
        """
        Convert Q expression to FilterNode for IR.

        Args:
            model_class: Model class for field resolution
            query: Optional query object for adding JOINs when FK paths are used

        Returns:
            FilterNode dict or None if empty
        """
        # Check if this is a combined Q with operations
        op = getattr(self, "_op", None)
        children = getattr(self, "_children", None)

        if op and children:
            # Recursively resolve children
            child_nodes = []
            for child in children:
                node = child.to_filter_node(model_class, query)
                if node is not None:
                    child_nodes.append(node)

            if not child_nodes:
                return None

            if op == "and":
                if len(child_nodes) == 1:
                    return child_nodes[0]
                return filter_and(*child_nodes)
            elif op == "or":
                if len(child_nodes) < 2:
                    # OR requires at least 2 conditions, return single or None
                    return child_nodes[0] if child_nodes else None
                return filter_or(*child_nodes)
            elif op == "not":
                if not child_nodes:
                    return None
                return filter_not(child_nodes[0])

        # Leaf Q expression
        return self._ensure_node(model_class, query)

    def __repr__(self) -> str:
        op = getattr(self, "_op", None)
        if op:
            children = getattr(self, "_children", [])
            if op == "not":
                return f"~{children[0]!r}"
            else:
                sep = f" {op.upper()} "
                return f"({sep.join(repr(c) for c in children)})"
        elif self._node:
            return f"Q({self._node})"
        else:
            return f"Q({', '.join(f'{k}={v!r}' for k, v in self._kwargs.items())})"


__all__ = ["Q"]

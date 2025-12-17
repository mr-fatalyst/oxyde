"""Joining mixin for query building."""

from __future__ import annotations

from collections.abc import Iterable
from typing import TYPE_CHECKING, Any

from oxyde.exceptions import FieldLookupError
from oxyde.queries.base import (
    TQuery,
    _collect_model_columns,
    _primary_key_meta,
    _resolve_registered_model,
)
from oxyde.queries.joins import _JoinDescriptor

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


class JoiningMixin:
    """Mixin providing join and prefetch capabilities."""

    # These attributes are defined in the base Query class
    model_class: type[OxydeModel]
    _join_specs: list[_JoinDescriptor]
    _prefetch_paths: list[str]

    def _clone(self: TQuery) -> TQuery:
        """Must be implemented by the main Query class."""
        raise NotImplementedError

    def join(self: TQuery, *paths: str) -> TQuery:
        """
        Add LEFT JOIN for related models.

        Args:
            *paths: Relation paths (e.g., "author", "author__profile")

        Examples:
            Post.objects.join("author")
            Post.objects.join("author", "category")
        """
        if not paths:
            raise ValueError("join() requires at least one relation path")
        clone = self._clone()
        for path in paths:
            clone._add_join_path(path)
        return clone

    def prefetch(self: TQuery, *paths: str) -> TQuery:
        """
        Prefetch related objects (separate queries).

        Args:
            *paths: Relation paths for one-to-many relations

        Examples:
            Author.objects.prefetch("posts")
        """
        if not paths:
            raise ValueError("prefetch() requires at least one relation path")
        clone = self._clone()
        for path in paths:
            if path not in clone._prefetch_paths:
                clone._prefetch_paths.append(path)
        return clone

    def _column_for_field(self, field: str) -> str:
        """Get database column name for a field."""
        self.model_class.ensure_field_metadata()
        meta = self.model_class._db_meta.field_metadata.get(field)
        if meta is not None:
            return meta.db_column
        return field

    def _column_mappings_for_fields(self, fields: Iterable[str]) -> dict[str, str]:
        """Get column mappings for fields where column differs from field name."""
        mappings: dict[str, str] = {}
        for field in fields:
            column = self._column_for_field(field)
            if column != field:
                mappings[field] = column
        return mappings

    def _add_join_path(self, path: str) -> None:
        """Add join descriptors for a relation path."""
        descriptors = self._compute_join_descriptors(path)
        existing = {spec.path for spec in self._join_specs}
        for descriptor in descriptors:
            if descriptor.path not in existing:
                self._join_specs.append(descriptor)
                existing.add(descriptor.path)

    def _compute_join_descriptors(self, path: str) -> list[_JoinDescriptor]:
        """Compute join descriptors for a relation path."""
        if not path:
            raise ValueError("join() path must be non-empty")
        segments = path.split("__")
        current_model = self.model_class
        parent_path: str | None = None
        parent_alias: str | None = None
        descriptors: list[_JoinDescriptor] = []
        prefix: list[str] = []

        for segment in segments:
            current_model.ensure_field_metadata()
            column_meta = current_model._db_meta.field_metadata.get(segment)
            if column_meta is None or column_meta.foreign_key is None:
                raise FieldLookupError(
                    f"{current_model.__name__}.{segment} is not a joinable relation"
                )
            target_model = _resolve_registered_model(column_meta.foreign_key.target)
            prefix.append(segment)
            path_key = "__".join(prefix)
            alias = path_key.replace("__", "_")
            pk_meta = _primary_key_meta(target_model)
            descriptor = _JoinDescriptor(
                path=path_key,
                alias=alias,
                result_prefix=path_key,
                parent_path=parent_path,
                parent_alias=parent_alias,
                attr_name=segment,
                source_column=column_meta.foreign_key.column_name,
                target_column=pk_meta.db_column,
                columns=_collect_model_columns(target_model),
                target_model=target_model,
                parent_model=current_model,
                nullable=column_meta.nullable,
            )
            descriptors.append(descriptor)
            current_model = target_model
            parent_path = path_key
            parent_alias = alias

        return descriptors

    def _join_specs_to_ir(self) -> list[dict[str, Any]]:
        """Convert join specs to IR format."""
        specs: list[dict[str, Any]] = []
        for descriptor in self._join_specs:
            specs.append(
                {
                    "path": descriptor.path,
                    "alias": descriptor.alias,
                    "parent": descriptor.parent_alias,
                    "table": descriptor.target_model.get_table_name(),
                    "source_column": descriptor.source_column,
                    "target_column": descriptor.target_column,
                    "result_prefix": descriptor.result_prefix,
                    "columns": [
                        {"field": field, "column": column}
                        for field, column in descriptor.columns
                    ],
                }
            )
        return specs

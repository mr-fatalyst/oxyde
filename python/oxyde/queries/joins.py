"""Internal descriptor for JOIN operations in queries.

_JoinDescriptor holds all metadata needed to generate a JOIN clause
and map results back to nested model instances.

Attributes:
    path: Dot-separated relation path (e.g., "author", "author.profile")
    alias: SQL table alias for this join (e.g., "t1", "t2")
    result_prefix: Prefix for result columns (e.g., "author__")
    parent_path: Path to parent join (None for direct joins)
    parent_alias: SQL alias of parent table
    attr_name: Attribute name on parent model (e.g., "author")
    source_column: FK column on source table (e.g., "author_id")
    target_column: PK column on target table (e.g., "id")
    columns: List of (field_name, db_column) for SELECT
    target_model: OxydeModel class being joined
    parent_model: OxydeModel class of the parent
    nullable: Whether this is a LEFT JOIN (True) or INNER JOIN (False)

Join Resolution:
    JoiningMixin.join("author") creates _JoinDescriptor:
    - Finds FK field on current model pointing to Author
    - Resolves source_column (author_id) and target_column (id)
    - Generates unique alias
    - Collects columns from target model

Nested Joins:
    join("author.profile") creates two descriptors:
    1. Post → Author (path="author")
    2. Author → Profile (path="author.profile", parent_path="author")

Usage:
    # Query uses these internally
    query = Post.objects.join("author", "category")
    for desc in query._join_specs:
        print(desc.path, desc.alias)  # "author" "t1", "category" "t2"
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel


@dataclass(slots=True)
class _JoinDescriptor:
    """Descriptor for a join operation."""

    path: str
    alias: str
    result_prefix: str
    parent_path: str | None
    parent_alias: str | None
    attr_name: str
    source_column: str
    target_column: str
    columns: list[tuple[str, str]]
    target_model: type[OxydeModel]
    parent_model: type[OxydeModel]
    nullable: bool


__all__ = ["_JoinDescriptor"]

"""Field configuration for OxydeModel with database metadata.

This module extends Pydantic's Field() with database-specific attributes.
OxydeFieldInfo stores both Pydantic validation rules and DB schema metadata.

Design:
    - OxydeFieldInfo inherits from pydantic.fields.FieldInfo
    - DB attributes stored as slots (not json_schema_extra) for type safety
    - Field() function is a factory returning OxydeFieldInfo

Attribute Categories:
    Primary Key:
        db_pk: Mark as PRIMARY KEY (auto-increment if int, None default)

    Indexing:
        db_index: Create single-column index
        db_index_name: Custom index name (default: ix_{table}_{column})
        db_index_method: Index type - btree, hash, gin, gist (PostgreSQL)
        db_unique: UNIQUE constraint

    Column Mapping:
        db_column: Override column name (default: field name)
        db_type: Override SQL type (e.g., "BIGSERIAL", "JSONB")
        db_default: SQL DEFAULT expression (e.g., "NOW()", "gen_random_uuid()")
        db_comment: SQL COMMENT ON COLUMN

    Foreign Keys:
        db_fk: Target field for FK (default: auto-detect PK of related model)
        db_on_delete: CASCADE | SET NULL | RESTRICT | NO ACTION
        db_on_update: CASCADE | SET NULL | RESTRICT | NO ACTION
        db_nullable: Override NULL/NOT NULL in DB. If None: infer from type hint
                     (X | None = nullable)

    Relations (virtual, not stored in DB):
        db_reverse_fk: Field name on related model for reverse lookup
        db_m2m: Many-to-many relation flag
        db_through: M2M junction table name

Example:
    from oxyde import OxydeModel, Field

    class User(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        email: str = Field(db_unique=True, db_index=True)
        name: str = Field(max_length=100)  # Pydantic validation
        bio: str | None = Field(default=None, db_type="TEXT")
        created_at: datetime = Field(db_default="NOW()")

        class Meta:
            is_table = True
"""

from __future__ import annotations

from typing import Any, Literal

from pydantic.fields import FieldInfo, PydanticUndefined


class OxydeFieldInfo(FieldInfo):
    """Extended FieldInfo with type-safe database metadata.

    This class extends Pydantic's FieldInfo to add database-specific metadata
    while maintaining full compatibility with Pydantic validation.

    All db_* parameters are stored as type-safe attributes (not in json_schema_extra),
    providing IDE autocomplete and type checking.
    """

    __slots__ = FieldInfo.__slots__ + (
        "db_pk",
        "db_index",
        "db_index_name",
        "db_index_method",
        "db_unique",
        "db_column",
        "db_type",
        "db_default",
        "db_comment",
        "db_fk",
        "db_on_delete",
        "db_on_update",
        "db_nullable",
        "db_reverse_fk",
        "db_m2m",
        "db_through",
    )

    def __init__(
        self,
        default: Any = PydanticUndefined,
        *,
        # === Database metadata ===
        db_pk: bool = False,
        db_index: bool = False,
        db_index_name: str | None = None,
        db_index_method: Literal["btree", "hash", "gin", "gist"] | None = None,
        db_unique: bool = False,
        db_column: str | None = None,
        db_type: str | None = None,
        db_default: str | None = None,
        db_comment: str | None = None,
        db_fk: str | None = None,
        db_on_delete: Literal[
            "CASCADE", "SET NULL", "RESTRICT", "NO ACTION"
        ] = "RESTRICT",
        db_on_update: Literal[
            "CASCADE", "SET NULL", "RESTRICT", "NO ACTION"
        ] = "CASCADE",
        db_nullable: bool | None = None,
        db_reverse_fk: str | None = None,
        db_m2m: bool = False,
        db_through: str | None = None,
        **kwargs: Any,  # All Pydantic parameters: ge, le, alias, description, etc.
    ) -> None:
        """Initialize OxydeFieldInfo with Pydantic and database metadata.

        Args:
            default: Default value for the field
            db_pk: Mark this field as primary key
            db_index: Create an index on this field
            db_index_name: Custom index name
            db_index_method: Index method (btree, hash, gin, gist)
            db_unique: Add unique constraint
            db_column: Database column name (overrides field name)
            db_type: SQL type override (e.g., "BIGSERIAL", "TEXT", "JSONB")
            db_default: SQL DEFAULT expression (e.g., "NOW()", "0")
            db_comment: SQL COMMENT for the column
            db_fk: Foreign key target. For model type hint: field name (default: PK).
                   For primitive type hint: "table.column" format.
            db_on_delete: Foreign key ON DELETE action
            db_on_update: Foreign key ON UPDATE action
            db_nullable: Override NULL/NOT NULL in DB. None = infer from type hint.
            db_reverse_fk: Reverse FK field name for loading relations
            db_m2m: Mark this field as many-to-many relation
            db_through: M2M through table name
            **kwargs: All other Pydantic FieldInfo parameters
        """
        # Initialize Pydantic FieldInfo (validators, alias, constraints)
        super().__init__(default=default, **kwargs)

        # Store DB metadata as type-safe attributes
        self.db_pk = db_pk
        self.db_index = db_index
        self.db_index_name = db_index_name
        self.db_index_method = db_index_method
        self.db_unique = db_unique
        self.db_column = db_column
        self.db_type = db_type
        self.db_default = db_default
        self.db_comment = db_comment
        self.db_fk = db_fk
        self.db_on_delete = db_on_delete
        self.db_on_update = db_on_update
        self.db_nullable = db_nullable
        self.db_reverse_fk = db_reverse_fk
        self.db_m2m = db_m2m
        self.db_through = db_through


def Field(
    default: Any = PydanticUndefined,
    **kwargs: Any,
) -> OxydeFieldInfo:
    """Create Oxyde field with Pydantic and database metadata.

    Combines Pydantic validation (ge, le, alias, etc.) with database metadata
    (db_pk, db_index, etc.) in a single type-safe API.

    Examples:
        Basic fields:
        >>> id: int | None = Field(default=None, db_pk=True)
        >>> email: str = Field(db_unique=True, db_index=True)
        >>> age: int = Field(ge=18, le=150)  # Pydantic validators

        With column mapping:
        >>> created_at: datetime = Field(
        ...     alias="createdAt",              # JSON API
        ...     db_column="created_timestamp",  # Database
        ...     db_default="NOW()"
        ... )

        Foreign keys (type hint must be OxydeModel, db_fk specifies target):
        >>> author: Author | None = Field(default=None, db_on_delete="CASCADE")
        >>> author: Author | None = Field(db_fk="uuid", db_on_delete="CASCADE")

        Reverse relations:
        >>> posts: list[Post] = Field(db_reverse_fk="author")

        Many-to-many:
        >>> tags: list[Tag] = Field(db_m2m=True, db_through="PostTag")

    Returns:
        OxydeFieldInfo instance with both Pydantic and database metadata
    """
    return OxydeFieldInfo(default=default, **kwargs)


__all__ = ["OxydeFieldInfo", "Field"]

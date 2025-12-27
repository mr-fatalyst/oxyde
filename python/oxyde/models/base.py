"""OxydeModel base class for defining database models.

This module provides the foundation of the Oxyde ORM model system. It defines
OxydeModel, a Pydantic v2 BaseModel subclass with database mapping capabilities.

Architecture:
    OxydeModelMeta (metaclass)
        └── Intercepts field access for DSL queries (User.name → QueryField)
        └── Tracks pending FK fields for lazy resolution
        └── Creates ModelFieldDescriptor for each model field

    OxydeModel (base class)
        └── Inherits from pydantic.BaseModel for validation
        └── Uses extra="ignore" to skip JOIN fields (author__id, author__name)
        └── Auto-registers in global model registry if Meta.is_table = True
        └── Provides QueryManager via Model.objects for CRUD operations
        └── Instance methods: save(), delete(), refresh()
        └── Lifecycle hooks: pre_save, post_save, pre_delete, post_delete

Key Components:
    _db_meta (ClassVar[ModelMeta]):
        Stores table-level metadata: table_name, schema, indexes, constraints.
        Also holds field_metadata dict mapping field names to ColumnMeta.

    objects (ClassVar[QueryManager]):
        Django-style manager for queries. Auto-created in __init_subclass__.
        Example: User.objects.filter(age__gte=18).all()

    ensure_field_metadata():
        Lazily parses field annotations and db_* attributes into ColumnMeta.
        Called automatically before queries to avoid circular imports.

    _resolve_fk_fields():
        Handles FK type annotations (field: Author) by auto-adding
        the {field}_{pk} column (author_id: int) to the model.

Internal Flow:
    1. Class definition triggers OxydeModelMeta.__new__()
    2. __init_subclass__() extracts Meta, creates _db_meta, registers table
    3. First query calls ensure_field_metadata() → _parse_field_tags()
    4. Field metadata cached in _db_meta.field_metadata for reuse

Example:
    class User(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str = Field(db_index=True)
        email: str = Field(db_unique=True)

        class Meta:
            is_table = True
            table_name = "users"

        async def pre_save(self, *, is_create: bool, update_fields=None):
            if is_create:
                self.created_at = datetime.utcnow()

    # Class-level DSL access (for queries)
    User.name  # Returns QueryField for building filters

    # Instance methods
    user = User(name="Alice", email="alice@example.com")
    await user.save()  # INSERT (pre_save → INSERT → post_save)
    await user.refresh()  # Re-fetch from DB
    await user.delete()  # DELETE (pre_delete → DELETE → post_delete)
"""

from __future__ import annotations

import sys
import typing as typing_module
import warnings
from collections.abc import Iterable
from typing import (
    TYPE_CHECKING,
    Any,
    ClassVar,
    get_args,
    get_origin,
    get_type_hints,
)

from pydantic import BaseModel, ConfigDict
from pydantic._internal._model_construction import ModelMetaclass
from pydantic.fields import FieldInfo, PydanticUndefined

from oxyde.core import register_validator

if TYPE_CHECKING:
    from oxyde.queries.base import SupportsExecute
    from oxyde.queries.manager import QueryManager

from oxyde.exceptions import FieldError, ManagerError, NotFoundError
from oxyde.models.metadata import (
    ColumnMeta,
    ForeignKeyInfo,
    ModelMeta,
    RelationDescriptorBase,
    RelationInfo,
)
from oxyde.models.registry import register_table
from oxyde.models.serializers import _dump_update_data
from oxyde.models.utils import _extract_max_length, _unpack_annotated, _unwrap_optional


def _get_pk_field_name(model_cls: type) -> str:
    """Get the primary key field name from an OxydeModel class.

    Args:
        model_cls: The OxydeModel class to inspect

    Returns:
        Primary key field name, defaults to "id" if not found
    """
    # Import here to avoid circular dependency
    from oxyde.models.field import OxydeFieldInfo

    # Check if model has already processed metadata
    if hasattr(model_cls, "_db_meta") and model_cls._db_meta.field_metadata:
        for field_name, meta in model_cls._db_meta.field_metadata.items():
            if meta.primary_key:
                return field_name

    # Fallback: check model_fields directly for db_pk=True
    if hasattr(model_cls, "model_fields"):
        for field_name, field_info in model_cls.model_fields.items():
            if isinstance(field_info, OxydeFieldInfo) and getattr(
                field_info, "db_pk", False
            ):
                return field_name

    # Default to "id" if no PK found
    return "id"


class OxydeModelMeta(ModelMetaclass):
    """Custom metaclass that exposes model fields as DSL descriptors."""

    def __new__(mcs, name: str, bases: tuple, namespace: dict, **kwargs: Any):
        """Create new OxydeModel class with auto-configured relation fields."""
        # Import here to avoid circular dependency
        from oxyde.models.field import OxydeFieldInfo

        # Track FK fields that need column generation (resolved lazily)
        pending_fk_fields: list[tuple[str, Any, FieldInfo | None]] = []

        # Auto-add default_factory=list for relation fields BEFORE Pydantic
        # This ensures fields with db_reverse_fk or db_m2m are not required
        annotations = namespace.get("__annotations__", {})
        for field_name, annotation in list(annotations.items()):
            field_info = namespace.get(field_name)
            if isinstance(field_info, OxydeFieldInfo):
                has_reverse_fk = getattr(field_info, "db_reverse_fk", None)
                has_m2m = getattr(field_info, "db_m2m", False)
                if has_reverse_fk or has_m2m:
                    # Auto-add default_factory=list if no default specified
                    if (
                        field_info.default is PydanticUndefined
                        and field_info.default_factory is None
                    ):
                        field_info.default_factory = list

            # Detect FK fields (annotation is OxydeModel subclass)
            # Skip reverse FK and M2M fields
            is_reverse_fk = isinstance(field_info, OxydeFieldInfo) and getattr(
                field_info, "db_reverse_fk", None
            )
            is_m2m = isinstance(field_info, OxydeFieldInfo) and getattr(
                field_info, "db_m2m", False
            )
            if not is_reverse_fk and not is_m2m:
                inner_type, _ = _unwrap_optional(annotation)
                # Check if it's a model class (will be resolved later if forward ref)
                if isinstance(inner_type, type) and issubclass(inner_type, BaseModel):
                    pending_fk_fields.append((field_name, annotation, field_info))
                elif isinstance(inner_type, str):
                    # Forward reference - also track for later resolution
                    pending_fk_fields.append((field_name, annotation, field_info))

        # Store pending FK fields for lazy resolution (__dunder__ avoids Pydantic)
        namespace["__pending_fk_fields__"] = pending_fk_fields
        namespace["__fk_fields_resolved__"] = False

        cls = super().__new__(mcs, name, bases, namespace, **kwargs)

        # Resolve FK fields AFTER Pydantic completes (model_fields is now populated)
        # This also tries to resolve any previously pending models
        from oxyde.models.registry import resolve_pending_fk

        resolve_pending_fk()

        return cls

    def __getattr__(cls, item: str) -> Any:  # type: ignore[override]
        return super().__getattr__(item)  # type: ignore[misc]


class OxydeModel(BaseModel, metaclass=OxydeModelMeta):
    """Base class for Oxyde ORM models."""

    model_config = ConfigDict(
        ignored_types=(RelationDescriptorBase,),
    )
    _db_meta: ClassVar[ModelMeta]
    objects: ClassVar[QueryManager]
    _is_table: ClassVar[bool] = False

    def __init__(self, **data: Any) -> None:
        # Ensure FK fields are in model_fields before Pydantic validates
        self.__class__.ensure_field_metadata()
        super().__init__(**data)

    def __init_subclass__(cls, **kwargs: Any) -> None:
        super().__init_subclass__(**kwargs)

        module = sys.modules.get(cls.__module__)
        if module is not None:
            setattr(module, cls.__name__, cls)

        # Extract Meta attributes
        meta_dict: dict[str, Any] = {}
        meta_cls = cls.__dict__.get("Meta")
        if meta_cls is not None:
            for attr in dir(meta_cls):
                if not attr.startswith("_"):
                    meta_dict[attr] = getattr(meta_cls, attr)

        # Determine whether this class represents a table
        is_table = bool(meta_dict.get("is_table", False))
        cls._is_table = is_table

        # Set table name if not provided
        if "table_name" not in meta_dict and is_table:
            meta_dict["table_name"] = cls.__name__.lower()

        known_meta_keys = {
            "is_table",
            "table_name",
            "schema",
            "comment",
            "indexes",
            "unique_together",
            "constraints",
        }
        indexes = list(meta_dict.get("indexes", []) or [])
        unique_together = [
            tuple(group) if not isinstance(group, tuple) else group
            for group in (meta_dict.get("unique_together", []) or [])
        ]
        constraints = list(meta_dict.get("constraints", []) or [])
        extra_meta = {
            key: value for key, value in meta_dict.items() if key not in known_meta_keys
        }

        cls._db_meta = ModelMeta(
            table_name=meta_dict.get("table_name"),
            schema=meta_dict.get("schema"),
            comment=meta_dict.get("comment"),
            indexes=indexes,
            unique_together=unique_together,
            constraints=constraints,
            extra=extra_meta,
        )

        relation_descriptors: list[tuple[str, Any]] = [
            (name, value)
            for name, value in cls.__dict__.items()
            if getattr(value, "_is_relation_descriptor", False)
        ]

        # Initialize manager (async by default)
        from oxyde.queries.manager import (  # local import to avoid circular dependency
            QueryManager,
        )

        cls.objects = QueryManager(cls)

        # Register validator with Rust core for server-side row validation
        model_key = f"{cls.__module__}.{cls.__qualname__}"
        validator = getattr(cls, "__pydantic_validator__", None)
        if validator is not None:
            try:
                register_validator(model_key, validator)
            except RuntimeError as exc:  # pragma: no cover - optional core feature
                warnings.warn(
                    f"Failed to register validator for {model_key}: {exc}",
                    RuntimeWarning,
                    stacklevel=2,
                )

        if is_table:
            register_table(cls, overwrite=True)
        else:
            cls._db_meta.field_metadata = {}
            cls._db_meta.relations = {}

        if relation_descriptors and is_table:
            for name, descriptor in relation_descriptors:
                descriptor.contribute_to_model(cls, name)

    @classmethod
    def ensure_field_metadata(cls) -> None:
        """Ensure field metadata has been parsed for table models."""
        # First resolve FK fields if not done yet
        fk_resolved = getattr(cls, "__fk_fields_resolved__", False)
        pending_fk = getattr(cls, "__pending_fk_fields__", [])
        if not fk_resolved and pending_fk:
            cls._resolve_fk_fields()
        if cls._is_table and not cls._db_meta.field_metadata:
            cls._parse_field_tags()
            # Compute col_types for IR after field_metadata is populated
            cls._compute_col_types()

    @classmethod
    def _compute_col_types(cls) -> None:
        """Compute IR type hints from field metadata (cached in _db_meta.col_types)."""
        from oxyde.core.ir_types import get_ir_type

        if cls._db_meta.col_types is not None:
            return  # Already computed

        col_types: dict[str, str] = {}
        for meta in cls._db_meta.field_metadata.values():
            # Skip virtual relation fields
            if meta.extra.get("reverse_fk") or meta.extra.get("m2m"):
                continue
            # Use explicit db_type if specified, otherwise infer from python_type
            if meta.db_type:
                col_types[meta.db_column] = meta.db_type.upper()
            else:
                ir_type = get_ir_type(meta.python_type)
                if ir_type:
                    col_types[meta.db_column] = ir_type

        cls._db_meta.col_types = col_types if col_types else None

    @classmethod
    def _resolve_fk_fields(cls) -> None:
        """Resolve pending FK fields and add {field}_{pk} columns to the model."""
        if getattr(cls, "__fk_fields_resolved__", False):
            return
        cls.__fk_fields_resolved__ = True

        pending_fk = getattr(cls, "__pending_fk_fields__", [])
        if not pending_fk:
            return

        from oxyde.models.field import Field, OxydeFieldInfo
        from oxyde.queries.manager import QueryManager
        from oxyde.queries.select import Query

        # Get type hints to resolve forward references
        module = sys.modules.get(cls.__module__)
        globalns = vars(module) if module is not None else {}
        combined_globalns = {**typing_module.__dict__, **globalns}
        combined_globalns.setdefault("OxydeModel", OxydeModel)
        combined_globalns.setdefault("ModelMeta", ModelMeta)
        combined_globalns.setdefault("ColumnMeta", ColumnMeta)
        combined_globalns.setdefault("ForeignKeyInfo", ForeignKeyInfo)
        combined_globalns.setdefault("Query", Query)
        combined_globalns.setdefault("QueryManager", QueryManager)

        try:
            hints = get_type_hints(
                cls,
                include_extras=True,
                globalns=combined_globalns,
                localns=dict(cls.__dict__),
            )
        except NameError:
            # Forward reference not yet resolvable - skip for now
            cls.__fk_fields_resolved__ = False
            return

        fields_to_add: list[tuple[str, type, FieldInfo]] = []

        for field_name, _annotation, field_info in pending_fk:
            # Get resolved type hint
            hint = hints.get(field_name)
            if hint is None:
                continue

            base_hint, _ = _unpack_annotated(hint)
            inner_type, is_optional = _unwrap_optional(base_hint)

            # Check if it's an OxydeModel subclass
            if not (
                isinstance(inner_type, type) and issubclass(inner_type, OxydeModel)
            ):
                continue

            # Get target field: db_fk or auto-detect PK
            target_model = inner_type
            db_fk = None
            if isinstance(field_info, OxydeFieldInfo):
                db_fk = getattr(field_info, "db_fk", None)
            target_field_name = db_fk or _get_pk_field_name(target_model)

            # Get target field type from target model
            target_field_info = target_model.model_fields.get(target_field_name)
            if target_field_info is None:
                fk_type: type = int  # fallback
            else:
                fk_annotation = target_field_info.annotation
                fk_type_inner, _ = _unwrap_optional(fk_annotation)
                fk_type = fk_type_inner if isinstance(fk_type_inner, type) else int

            # Build FK column name: db_column or {field_name}_{target_field}
            db_column = None
            db_nullable = None
            if isinstance(field_info, OxydeFieldInfo):
                db_column = getattr(field_info, "db_column", None)
                db_nullable = getattr(field_info, "db_nullable", None)
            fk_column_name = db_column or f"{field_name}_{target_field_name}"

            # Check if this field already exists (avoid duplicates)
            if fk_column_name in cls.model_fields:
                continue

            # Determine FK column type (nullable if original FK field is optional)
            # db_nullable from virtual FK field applies to the real column
            if is_optional:
                fk_column_type = fk_type | None
                fk_default = None
            else:
                fk_column_type = fk_type
                fk_default = PydanticUndefined

            # Pass db_nullable to the synthetic FK column field
            fields_to_add.append(
                (
                    fk_column_name,
                    fk_column_type,
                    Field(default=fk_default, db_nullable=db_nullable),
                )
            )

        if not fields_to_add:
            return

        # Add fields to model
        added_fields: list[str] = []
        for fk_column_name, fk_column_type, fk_field_info in fields_to_add:
            cls.__annotations__[fk_column_name] = fk_column_type
            # Preserve OxydeFieldInfo with db_nullable
            fk_field_info.annotation = fk_column_type
            cls.__pydantic_fields__[fk_column_name] = fk_field_info
            added_fields.append(fk_column_name)

        # Rebuild model to include new fields
        # May fail if other forward refs are not yet resolvable
        from pydantic.errors import PydanticUndefinedAnnotation

        try:
            cls.model_rebuild(force=True)
        except PydanticUndefinedAnnotation:
            # Forward ref not resolvable yet - revert and retry later
            for fk_column_name in added_fields:
                cls.__annotations__.pop(fk_column_name, None)
                cls.__pydantic_fields__.pop(fk_column_name, None)
            cls.__fk_fields_resolved__ = False

    @classmethod
    def _parse_field_tags(cls) -> None:
        """Parse Annotated field tags and store them in metadata."""
        cls._db_meta.field_metadata = {}
        if not cls._is_table:
            return

        module = sys.modules.get(cls.__module__)
        globalns = vars(module) if module is not None else {}
        combined_globalns = {**typing_module.__dict__, **globalns}
        combined_globalns.setdefault("ModelMeta", ModelMeta)
        combined_globalns.setdefault("ColumnMeta", ColumnMeta)
        combined_globalns.setdefault("ForeignKeyInfo", ForeignKeyInfo)
        combined_globalns.setdefault("OxydeModel", OxydeModel)
        # Local imports to avoid circular dependency (OxydeModel ↔ Query)
        from oxyde.queries.manager import QueryManager
        from oxyde.queries.select import Query

        combined_globalns.setdefault("Query", Query)
        combined_globalns.setdefault("QueryManager", QueryManager)
        hints = get_type_hints(
            cls,
            include_extras=True,
            globalns=combined_globalns,
            localns=dict(cls.__dict__),
        )

        for field_name, model_field in cls.model_fields.items():
            # Get type hint for nullable detection and FK inference
            hint = hints.get(field_name, model_field.annotation)
            base_hint, _ = _unpack_annotated(hint)  # annotations not used anymore
            python_type, optional_flag = _unwrap_optional(base_hint)

            # Extract db_* attributes from OxydeFieldInfo (or use defaults)
            from oxyde.models.field import OxydeFieldInfo

            def get_db_attr(
                field: FieldInfo, attr_name: str, default: Any = None
            ) -> Any:
                """Extract db_* attribute from OxydeFieldInfo or return default."""
                if isinstance(field, OxydeFieldInfo):
                    return getattr(field, attr_name, default)
                return default

            # Read db_* attributes (type-safe access)
            db_pk = get_db_attr(model_field, "db_pk", False)
            db_index = get_db_attr(model_field, "db_index", False)
            db_index_name = get_db_attr(model_field, "db_index_name", None)
            db_index_method = get_db_attr(model_field, "db_index_method", None)
            db_unique = get_db_attr(model_field, "db_unique", False)
            db_column = get_db_attr(model_field, "db_column", None)
            db_type = get_db_attr(model_field, "db_type", None)
            db_default = get_db_attr(model_field, "db_default", None)
            db_comment = get_db_attr(model_field, "db_comment", None)
            db_fk = get_db_attr(model_field, "db_fk", None)
            db_on_delete = get_db_attr(model_field, "db_on_delete", "RESTRICT")
            db_on_update = get_db_attr(model_field, "db_on_update", "CASCADE")
            db_nullable = get_db_attr(model_field, "db_nullable", None)
            db_reverse_fk = get_db_attr(model_field, "db_reverse_fk", None)
            db_m2m = get_db_attr(model_field, "db_m2m", False)
            db_through = get_db_attr(model_field, "db_through", None)

            # Foreign Key detection
            # FK is defined by type annotation: field type must be OxydeModel
            # db_fk parameter specifies target field (defaults to PK)
            fk_info: ForeignKeyInfo | None = None
            is_fk = isinstance(python_type, type) and issubclass(
                python_type, OxydeModel
            )

            # Determine nullable:
            # 1. If db_nullable explicitly set — use it
            # 2. Otherwise — infer from type hint (X | None = nullable)
            if db_nullable is not None:
                nullable = db_nullable
            else:
                nullable = optional_flag

            default_value = model_field.default
            default_factory = getattr(model_field, "default_factory", None)

            # Allow None default for optional fields (non-FK only)
            # Only override if db_nullable was not explicitly set
            if (
                not is_fk
                and not nullable
                and db_nullable is None  # don't override explicit db_nullable
                and not model_field.is_required()
                and default_value is None
                and default_value is not PydanticUndefined
            ):
                nullable = True

            if is_fk:
                target_key = f"{python_type.__module__}.{python_type.__qualname__}"

                # Determine target field: db_fk or auto-detect PK
                if db_fk:
                    # Explicit target field specified
                    target_field = db_fk
                else:
                    # Auto-detect PK field of target model
                    target_field = _get_pk_field_name(python_type)

                # FK column name: db_column override or {field_name}_{target_field}
                column_name = db_column or f"{field_name}_{target_field}"

                fk_info = ForeignKeyInfo(
                    target=target_key,
                    column_name=column_name,
                    target_field=target_field,
                    nullable=nullable,
                    on_delete=db_on_delete,
                    on_update=db_on_update,
                )

            # Extract max_length for varchar fields
            # Note: db_type is only set if user specifies Field(db_type="...")
            # Type inference happens at schema extraction time.
            max_length = _extract_max_length(model_field)

            # Determine actual DB column name
            if fk_info is not None:
                # For FK fields, use FK column name
                db_column_name = fk_info.column_name
            else:
                # For regular fields, use db_column or field name
                db_column_name = db_column or field_name

            # Build extras dict for additional metadata
            extras_filtered: dict[str, Any] = {}
            if db_reverse_fk:
                extras_filtered["reverse_fk"] = db_reverse_fk
            if db_m2m:
                extras_filtered["m2m"] = True
                if db_through:
                    extras_filtered["through"] = db_through

            # Populate _db_meta.relations for reverse FK and M2M fields
            if db_reverse_fk or db_m2m:
                # Extract target model from list[X] type
                target_name: str | None = None
                origin = get_origin(python_type)
                if origin is list:
                    list_args = get_args(python_type)
                    if list_args:
                        element_type = list_args[0]
                        if isinstance(element_type, str):
                            target_name = element_type
                        elif isinstance(element_type, type):
                            target_name = element_type.__name__

                if target_name:
                    if db_reverse_fk:
                        # One-to-many (reverse FK) relation
                        cls._db_meta.relations[field_name] = RelationInfo(
                            name=field_name,
                            kind="one_to_many",
                            target=target_name,
                            remote_field=db_reverse_fk,
                            through=None,
                        )
                    elif db_m2m:
                        # Many-to-many relation
                        cls._db_meta.relations[field_name] = RelationInfo(
                            name=field_name,
                            kind="many_to_many",
                            target=target_name,
                            remote_field=None,
                            through=db_through,
                        )

            column_meta = ColumnMeta(
                name=field_name,
                db_column=db_column_name,
                python_type=python_type,
                nullable=nullable,
                db_type=db_type,
                index=db_index,
                index_name=db_index_name,
                index_method=db_index_method,
                unique=db_unique,
                primary_key=db_pk,
                comment=db_comment,
                default=default_value,
                default_factory=default_factory,
                db_default=db_default,
                max_length=max_length,
                foreign_key=fk_info,
                checks=[],  # Field-level checks not supported yet
                extra=extras_filtered,
            )
            cls._db_meta.field_metadata[field_name] = column_meta

    @classmethod
    def get_table_name(cls) -> str:
        """Get the table name for this model."""
        return cls._db_meta.table_name or cls.__name__.lower()

    @classmethod
    def _get_primary_key_field(cls) -> str | None:
        """Get primary key field name from model metadata."""
        cls.ensure_field_metadata()
        for field_name, meta in cls._db_meta.field_metadata.items():
            if meta.primary_key:
                return field_name
        return None

    # =========================================================================
    # Lifecycle Hooks
    # =========================================================================

    async def pre_save(
        self,
        *,
        is_create: bool,
        update_fields: set[str] | None = None,
    ) -> None:
        """Called before save/create. Override to add custom logic.

        Args:
            is_create: True if this is a new record, False if updating.
            update_fields: Set of field names being updated, or None for all fields.

        Example:
            async def pre_save(self, *, is_create: bool, update_fields: set[str] | None = None) -> None:
                if is_create:
                    self.created_at = datetime.now()
                if self.password and not self.password.startswith("$2b$"):
                    self.password = await hash_password(self.password)
        """
        pass

    async def post_save(
        self,
        *,
        is_create: bool,
        update_fields: set[str] | None = None,
    ) -> None:
        """Called after save/create. Override to add custom logic.

        Args:
            is_create: True if this was a new record, False if updated.
            update_fields: Set of field names that were updated, or None for all fields.

        Example:
            async def post_save(self, *, is_create: bool, update_fields: set[str] | None = None) -> None:
                if is_create:
                    await send_welcome_email(self.email)
        """
        pass

    async def pre_delete(self) -> None:
        """Called before delete. Override to add custom logic.

        Example:
            async def pre_delete(self) -> None:
                await cleanup_user_files(self.id)
        """
        pass

    async def post_delete(self) -> None:
        """Called after delete. Override to add custom logic.

        Example:
            async def post_delete(self) -> None:
                await notify_admin(f"User {self.email} deleted")
        """
        pass

    # =========================================================================
    # Instance Methods
    # =========================================================================

    async def save(
        self,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
        update_fields: Iterable[str] | None = None,
    ) -> OxydeModel:
        manager = self.__class__.objects
        pk_field = self._get_primary_key_field()
        pk_value = getattr(self, pk_field) if pk_field else None
        is_create = not (pk_field and pk_value not in (None, PydanticUndefined))

        # Convert update_fields to set for hooks
        update_fields_set: set[str] | None = None
        if update_fields is not None:
            update_fields_set = set(update_fields)

        # Call pre_save hook
        await self.pre_save(is_create=is_create, update_fields=update_fields_set)

        if not is_create:
            # Update specified fields or all fields (except PK)
            if update_fields_set is not None:
                fields = update_fields_set
                # Validate that all specified fields exist
                model_field_names = set(self.__class__.model_fields.keys())
                invalid_fields = fields - model_field_names
                if invalid_fields:
                    inv = ", ".join(sorted(invalid_fields))
                    msg = f"Invalid update_fields for {self.__class__.__name__}: {inv}"
                    raise FieldError(msg)
            else:
                fields = set(self.__class__.model_fields.keys())
            fields.discard(pk_field)
            values = _dump_update_data(self, fields)
            if not values:
                # Call post_save even if no values to update (for consistency)
                await self.post_save(
                    is_create=is_create, update_fields=update_fields_set
                )
                return self

            # Use Manager.filter().update() for updating (returns list of updated rows)
            rows = await manager.filter(**{pk_field: pk_value}).update(
                client=client, using=using, **values
            )
            if not rows:
                cls_name = self.__class__.__name__
                raise NotFoundError(f"{cls_name} with {pk_field}={pk_value} not found")

            # Update instance from RETURNING * result
            self.__class__.ensure_field_metadata()
            col_to_field = {
                meta.db_column: field_name
                for field_name, meta in self.__class__._db_meta.field_metadata.items()
            }
            for col, value in rows[0].items():
                field_name = col_to_field.get(col, col)
                setattr(self, field_name, value)
        else:
            await manager.create(
                instance=self, client=client, using=using, _skip_hooks=True
            )

        # Call post_save hook
        await self.post_save(is_create=is_create, update_fields=update_fields_set)

        return self

    async def delete(
        self,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
    ) -> int:
        manager = self.__class__.objects
        pk_field = self._get_primary_key_field()
        if not pk_field:
            raise ManagerError("delete() requires a primary key")
        pk_value = getattr(self, pk_field)
        if pk_value is None:
            raise ManagerError(
                "delete() requires the instance to have a primary key value"
            )

        # Call pre_delete hook
        await self.pre_delete()

        # Use Manager.filter().delete() for deleting
        affected = await manager.filter(**{pk_field: pk_value}).delete(
            client=client, using=using
        )

        # Call post_delete hook
        await self.post_delete()

        return affected

    async def refresh(
        self,
        *,
        client: SupportsExecute | None = None,
        using: str | None = None,
    ) -> OxydeModel:
        """
        Reload this instance from the database.

        Args:
            client: Optional database client
            using: Optional database alias

        Returns:
            Self with updated values

        Raises:
            ManagerError: If model has no primary key
            NotFoundError: If instance not found in database

        Examples:
            user = await User.objects.get(id=42)
            # ... time passes, data may have changed ...
            await user.refresh()  # Reload from DB
        """
        pk_field = self._get_primary_key_field()
        if not pk_field:
            raise ManagerError("refresh() requires a primary key")
        pk_value = getattr(self, pk_field)
        if pk_value is None:
            raise ManagerError(
                "refresh() requires the instance to have a primary key value"
            )

        # Fetch fresh data from DB
        refreshed = await self.__class__.objects.get(
            client=client, using=using, **{pk_field: pk_value}
        )

        # Update all fields from the refreshed instance
        for field_name in self.model_fields.keys():
            setattr(self, field_name, getattr(refreshed, field_name))

        return self


__all__ = [
    "OxydeModel",
    "ModelMeta",
    "ColumnMeta",
    "ForeignKeyInfo",
    "RelationInfo",
    "RelationDescriptorBase",
]

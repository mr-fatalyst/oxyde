"""Global registry of OxydeModel table classes.

This module maintains a global dict mapping model keys to model classes.
Models are auto-registered when defined with Meta.is_table = True.

Model Key Format:
    "{module}.{qualname}" e.g., "myapp.models.User"

This ensures unique identification even for models with same class names
in different modules.

Functions:
    register_table(model, overwrite=False):
        Add model to registry. Raises ValueError if already registered
        and overwrite=False.

    unregister_table(model):
        Remove model from registry (no-op if not registered).

    registered_tables() -> dict[str, type[OxydeModel]]:
        Return copy of registry. Ensures field metadata is parsed.

    iter_tables() -> tuple[type[OxydeModel], ...]:
        Return tuple of registered model classes.

    clear_registry():
        Remove all models (used in tests for cleanup).

Auto-Registration:
    Models with Meta.is_table = True are automatically registered in
    OxydeModel.__init_subclass__(). This happens at class definition time.

    class User(OxydeModel):
        class Meta:
            is_table = True  # Auto-registers as "myapp.models.User"

Migration Integration:
    The migration system uses registered_tables() to discover models
    and generate schema diffs.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel

_TABLES: dict[str, type[OxydeModel]] = {}
_PENDING_FK_MODELS: set[type[OxydeModel]] = set()


def _model_key(model: type[OxydeModel]) -> str:
    return f"{model.__module__}.{model.__qualname__}"


def _resolve_pending_fk_models() -> None:
    """Try to resolve FK fields for all pending models."""
    resolved = set()
    for model in _PENDING_FK_MODELS:
        model.__fk_fields_resolved__ = False  # type: ignore[attr-defined]
        model._resolve_fk_fields()  # type: ignore[attr-defined]
        if getattr(model, "__fk_fields_resolved__", False):
            resolved.add(model)

    _PENDING_FK_MODELS.difference_update(resolved)


def register_table(model: type[OxydeModel], *, overwrite: bool = False) -> None:
    """Register an ORM model that represents a database table.

    Note: FK resolution is NOT done here because model_fields is not yet
    populated when this is called from __init_subclass__. FK resolution
    is triggered from OxydeModelMeta.__new__ after Pydantic completes.
    """
    key = _model_key(model)
    existing = _TABLES.get(key)
    if existing is model:
        return
    if existing is not None and not overwrite:
        raise ValueError(f"Table '{key}' is already registered")
    _TABLES[key] = model

    # Add to pending if has FK fields (will be resolved later)
    pending_fk = getattr(model, "__pending_fk_fields__", [])
    if pending_fk:
        _PENDING_FK_MODELS.add(model)


def unregister_table(model: type[OxydeModel]) -> None:
    """Remove a model from the registry if present."""
    _TABLES.pop(_model_key(model), None)


def registered_tables() -> dict[str, type[OxydeModel]]:
    """Return a copy of the registered table mapping."""
    tables = dict(_TABLES)
    for model in tables.values():
        if hasattr(model, "ensure_field_metadata"):
            model.ensure_field_metadata()  # type: ignore[attr-defined]
    return tables


def iter_tables() -> tuple[type[OxydeModel], ...]:
    """Return tuple of registered table classes."""
    tables = tuple(_TABLES.values())
    for model in tables:
        if hasattr(model, "ensure_field_metadata"):
            model.ensure_field_metadata()  # type: ignore[attr-defined]
    return tables


def clear_registry() -> None:
    """Reset the registry (intended for tests)."""
    _TABLES.clear()
    _PENDING_FK_MODELS.clear()


def resolve_pending_fk() -> None:
    """Resolve FK fields for all pending models.

    Called from OxydeModelMeta.__new__ after Pydantic completes model creation.
    At this point model_fields is populated and FK fields can be resolved.
    """
    _resolve_pending_fk_models()


__all__ = [
    "register_table",
    "unregister_table",
    "registered_tables",
    "iter_tables",
    "clear_registry",
    "resolve_pending_fk",
]

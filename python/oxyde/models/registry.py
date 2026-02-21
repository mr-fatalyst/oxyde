"""Global registry of Model table classes.

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

    registered_tables() -> dict[str, type[Model]]:
        Return copy of registry.

    iter_tables() -> tuple[type[Model], ...]:
        Return tuple of registered model classes.

    clear_registry():
        Remove all models (used in tests for cleanup).

    finalize_pending():
        Finalize all pending models: FK resolve + metadata parse + PK cache.

    assert_no_pending_models():
        Fail-fast check that all models are finalized.

Auto-Registration:
    Models with Meta.is_table = True are automatically registered in
    Model.__init_subclass__(). This happens at class definition time.

    class User(Model):
        class Meta:
            is_table = True  # Auto-registers as "myapp.models.User"

Migration Integration:
    The migration system uses registered_tables() to discover models
    and generate schema diffs.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from oxyde.models.base import Model

_TABLES: dict[str, type[Model]] = {}
_PENDING_MODELS: set[type[Model]] = set()


def _model_key(model: type[Model]) -> str:
    return f"{model.__module__}.{model.__qualname__}"


def _finalize_model(model: type[Model]) -> bool:
    """Try to fully finalize a single model: FK resolve → parse → col_types → PK cache.

    Returns True if model is fully finalized, False if it should be retried later.
    """
    # Step 1: Resolve FK fields if needed
    pending_fk = getattr(model, "__pending_fk_fields__", [])
    if pending_fk:
        fk_resolved = getattr(model, "__fk_fields_resolved__", False)
        if not fk_resolved:
            model.__fk_fields_resolved__ = False  # type: ignore[attr-defined]
            model._resolve_fk_fields()  # type: ignore[attr-defined]
            if not getattr(model, "__fk_fields_resolved__", False):
                return False

    # Step 2: Parse field tags → field_metadata
    if not model._db_meta.field_metadata:
        try:
            model._parse_field_tags()  # type: ignore[attr-defined]
        except NameError:
            return False

    # Step 3: Compute col_types for IR
    if model._db_meta.col_types is None:
        model._compute_col_types()  # type: ignore[attr-defined]

    # Step 4: Cache PK field
    if model._db_meta.pk_field is None:
        for field_name, meta in model._db_meta.field_metadata.items():
            if meta.primary_key:
                model._db_meta.pk_field = field_name
                model._db_meta.pk_column = meta.db_column
                break

    return True


def finalize_pending() -> None:
    """Finalize all pending models: FK resolve + metadata parse + PK cache.

    Called from OxydeModelMeta.__new__ after Pydantic completes model creation.
    Models that can't be finalized (forward refs not yet available) stay in
    _PENDING_MODELS and are retried on next class definition.
    """
    for model in list(_PENDING_MODELS):
        if _finalize_model(model):
            _PENDING_MODELS.discard(model)


def register_table(model: type[Model], *, overwrite: bool = False) -> None:
    """Register an ORM model that represents a database table.

    All table models are added to _PENDING_MODELS for finalization.
    Finalization is triggered from OxydeModelMeta.__new__ after Pydantic completes.
    """
    key = _model_key(model)
    existing = _TABLES.get(key)
    if existing is model:
        return
    if existing is not None:
        if not overwrite:
            raise ValueError(f"Table '{key}' is already registered")
        _PENDING_MODELS.discard(existing)
    _TABLES[key] = model
    _PENDING_MODELS.add(model)


def unregister_table(model: type[Model]) -> None:
    """Remove a model from the registry if present."""
    _TABLES.pop(_model_key(model), None)
    _PENDING_MODELS.discard(model)


def registered_tables() -> dict[str, type[Model]]:
    """Return a copy of the registered table mapping."""
    return dict(_TABLES)


def iter_tables() -> tuple[type[Model], ...]:
    """Return tuple of registered model classes."""
    return tuple(_TABLES.values())


def clear_registry() -> None:
    """Reset the registry (intended for tests)."""
    _TABLES.clear()
    _PENDING_MODELS.clear()


def assert_no_pending_models() -> None:
    """Verify all models are finalized. Call at startup after all imports.

    Raises RuntimeError if any models are still pending finalization.
    """
    if _PENDING_MODELS:
        names = ", ".join(m.__name__ for m in _PENDING_MODELS)
        raise RuntimeError(f"Models not finalized (unresolved forward refs?): {names}")


__all__ = [
    "register_table",
    "unregister_table",
    "registered_tables",
    "iter_tables",
    "clear_registry",
    "finalize_pending",
    "assert_no_pending_models",
]

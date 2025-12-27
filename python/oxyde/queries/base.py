"""Shared utilities, protocols, and helpers for query builders.

This module provides common functionality used across all query types
(SELECT, INSERT, UPDATE, DELETE).

Protocol:
    SupportsExecute: Interface for objects that can execute queries.
        - AsyncDatabase implements this
        - AsyncTransaction implements this
        Any object with async execute(ir: dict) -> bytes works.

Type Variables:
    TQuery: TypeVar bound to Query for method chaining with correct return types.

Helper Functions:
    _model_key(model_class) -> str:
        Returns "{module}.{qualname}" for model identification.

    _resolve_registered_model(key) -> type[OxydeModel]:
        Look up model by key or class name in registry.

    _primary_key_meta(model_class) -> ColumnMeta:
        Get metadata for primary key field.

    _collect_model_columns(model_class) -> list[(field, column)]:
        Get all (field_name, db_column) pairs.

    _map_values_to_columns(model_class, values) -> dict:
        Convert field names to database column names in a dict.

    _resolve_execution_client(using, client) -> SupportsExecute:
        Resolve connection from using="alias" or client object.
        Checks active transaction first.

    _resolve_pool_name(using, client) -> str:
        Get pool name for schema/explain operations.

Caching:
    _TYPE_ADAPTER_CACHE: Thread-safe cache for Pydantic TypeAdapter instances.
    Used for efficient list[Model] validation in fetch_models().
"""

from __future__ import annotations

import threading
from typing import (
    TYPE_CHECKING,
    Any,
    Protocol,
    TypeVar,
    runtime_checkable,
)

from pydantic import TypeAdapter

from oxyde.exceptions import FieldLookupError, ManagerError
from oxyde.models.registry import registered_tables

if TYPE_CHECKING:
    from oxyde.models.base import OxydeModel
    from oxyde.queries.select import Query

# Global cache for TypeAdapter instances (thread-safe)
_TYPE_ADAPTER_CACHE: dict[type, TypeAdapter] = {}
_TYPE_ADAPTER_LOCK = threading.Lock()


def _model_key(model_class: type[OxydeModel]) -> str:
    """Return fully qualified identifier for a model."""
    return f"{model_class.__module__}.{model_class.__qualname__}"


@runtime_checkable
class SupportsExecute(Protocol):
    """Protocol for objects that can execute queries."""

    async def execute(self, ir: dict[str, Any]) -> bytes: ...


# TypeVar for query builders (covariant for subclassing)
TQuery = TypeVar("TQuery", bound="Query")  # type: ignore


def _resolve_registered_model(model_key: str) -> type[OxydeModel]:
    """Resolve a model by its fully qualified key or simple class name."""
    tables = registered_tables()
    # Try exact match first
    model = tables.get(model_key)
    if model is not None:
        return model
    # Fallback: search by simple class name (for forward refs and test classes)
    for key, table_model in tables.items():
        if key.endswith(f".{model_key}") or table_model.__name__ == model_key:
            return table_model
    raise FieldLookupError(f"Related model '{model_key}' is not registered")


def _primary_key_meta(model_class: type[OxydeModel]):
    """Get primary key metadata from model."""
    model_class.ensure_field_metadata()
    for meta in model_class._db_meta.field_metadata.values():
        if meta.primary_key:
            return meta
    raise FieldLookupError(f"{model_class.__name__} has no primary key field")


def _build_col_types(model_class: type[OxydeModel]) -> dict[str, str] | None:
    """Get cached col_types mapping from model metadata."""
    model_class.ensure_field_metadata()
    return model_class._db_meta.col_types


def _collect_model_columns(model_class: type[OxydeModel]) -> list[tuple[str, str]]:
    """Collect all (field_name, db_column) pairs from model.

    Excludes virtual relation fields (db_reverse_fk, db_m2m) that don't
    have corresponding database columns.

    Also excludes virtual FK fields (author: User) since they share db_column
    with synthetic FK columns (author_id: int) - we use the synthetic ones
    to avoid type conflicts during hydration.
    """
    model_class.ensure_field_metadata()
    result = []
    for meta in model_class._db_meta.field_metadata.values():
        # Skip virtual relation fields (db_reverse_fk, db_m2m)
        if meta.extra.get("reverse_fk") or meta.extra.get("m2m"):
            continue
        # Skip virtual FK fields (author: User) - use synthetic author_id instead
        if meta.foreign_key is not None:
            continue
        result.append((meta.name, meta.db_column))
    return result


def _map_values_to_columns(
    model_class: type[OxydeModel],
    values: dict[str, Any],
) -> dict[str, Any]:
    """Map field names to database column names."""
    model_class.ensure_field_metadata()
    metadata = model_class._db_meta.field_metadata
    mapped: dict[str, Any] = {}
    for key, value in values.items():
        meta = metadata.get(key)
        column = meta.db_column if meta else key
        mapped[column] = value
    return mapped


async def _resolve_execution_client(
    using: str | None,
    client: SupportsExecute | None,
) -> SupportsExecute:
    """Resolve the execution client from using/client parameters."""
    if client is not None and using is not None:
        raise ManagerError("Provide either 'client' or 'using', not both")
    if client is not None:
        return client
    from oxyde.db.registry import get_connection
    from oxyde.db.transaction import get_active_transaction

    alias = using or "default"
    active_tx = get_active_transaction(alias)
    if active_tx is not None:
        return active_tx
    return await get_connection(alias)


def _resolve_pool_name(
    using: str | None,
    client: SupportsExecute | None,
) -> str:
    """
    Resolve pool name from using or client parameters.

    Args:
        using: Database alias
        client: Database client (AsyncDatabase or AsyncTransaction)

    Returns:
        Pool name string

    Raises:
        ManagerError: If both using and client are provided
    """
    if client is not None and using is not None:
        raise ManagerError("Provide either 'client' or 'using', not both")

    if client is not None:
        # Try to get pool name from client
        from oxyde.db.pool import AsyncDatabase
        from oxyde.db.transaction import AsyncTransaction

        if isinstance(client, AsyncDatabase):
            return client.name
        elif isinstance(client, AsyncTransaction):
            return client._database.name
        else:
            # Unknown client type, try to get name attribute
            if hasattr(client, "name"):
                return client.name
            elif hasattr(client, "_database") and hasattr(client._database, "name"):
                return client._database.name
            else:
                ctype = type(client).__name__
                raise ManagerError(f"Cannot determine pool name from client: {ctype}")

    # Use alias
    return using or "default"


__all__ = [
    "SupportsExecute",
    "TQuery",
    "_model_key",
    "_resolve_registered_model",
    "_primary_key_meta",
    "_collect_model_columns",
    "_map_values_to_columns",
    "_resolve_execution_client",
    "_resolve_pool_name",
    "_TYPE_ADAPTER_CACHE",
    "_TYPE_ADAPTER_LOCK",
]

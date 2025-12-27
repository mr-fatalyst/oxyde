"""Intermediate Representation (IR) builders for Rust query execution.

This module provides functions to construct query IR dicts that are
serialized to MessagePack and sent to the Rust core for SQL generation.

IR Protocol:
    Every IR payload has:
        proto: Version number (currently 1)
        op: Operation type ("select", "insert", "update", "delete", "raw")
        table: Target table name

Filter Tree:
    WHERE clauses are represented as a tree of FilterNode dicts:

    Condition (leaf):
        {"type": "condition", "field": "age", "operator": ">=", "value": 18}

    AND (branch):
        {"type": "and", "conditions": [node1, node2, ...]}

    OR (branch):
        {"type": "or", "conditions": [node1, node2]}

    NOT (branch):
        {"type": "not", "condition": node}

Functions:
    filter_condition(field, operator, value) → FilterNode
    filter_and(*nodes) → FilterNode
    filter_or(*nodes) → FilterNode
    filter_not(node) → FilterNode

    build_select_ir(...) → dict:
        SELECT with columns, filter_tree, order_by, limit, joins, aggregates, etc.

    build_insert_ir(...) → dict:
        INSERT with values or bulk_values, pk_column for RETURNING.

    build_update_ir(...) → dict:
        UPDATE with values and filter_tree, or bulk_update for CASE WHEN.

    build_delete_ir(...) → dict:
        DELETE with filter_tree.

    build_raw_sql_ir(sql, params) → dict:
        Raw SQL escape hatch.

Example IR (SELECT):
    {
        "proto": 1,
        "op": "select",
        "table": "users",
        "cols": ["id", "name", "email"],
        "col_types": {"id": "int", "name": "str", "email": "str"},
        "filter_tree": {"type": "condition", "field": "active", "operator": "=", "value": true},
        "order_by": [("created_at", "desc")],
        "limit": 10
    }

Rust Processing:
    IR bytes → oxyde-codec::QueryIR → oxyde-query::build_sql() → SQL string
    SQL → oxyde-driver::execute_query() → rows → MessagePack bytes → Python
"""

from __future__ import annotations

from collections.abc import MutableMapping, Sequence
from typing import Any

IR_PROTO_VERSION = 1

FilterNode = MutableMapping[str, Any]


def filter_condition(
    field: str,
    operator: str,
    value: Any,
    *,
    column: str | None = None,
) -> FilterNode:
    """Create a simple condition node."""
    payload: FilterNode = {
        "type": "condition",
        "field": field,
        "operator": operator,
        "value": value,
    }
    if column is not None and column != field:
        payload["column"] = column
    return payload


def filter_and(*nodes: FilterNode) -> FilterNode:
    """Combine nodes with logical AND."""
    normalized = [_ensure_filter_node(node) for node in nodes if node]
    if not normalized:
        raise ValueError("filter_and() requires at least one condition")
    if len(normalized) == 1:
        return normalized[0]
    return {"type": "and", "conditions": normalized}


def filter_or(*nodes: FilterNode) -> FilterNode:
    """Combine nodes with logical OR."""
    normalized = [_ensure_filter_node(node) for node in nodes if node]
    if len(normalized) < 2:
        raise ValueError("filter_or() requires at least two conditions")
    return {"type": "or", "conditions": normalized}


def filter_not(node: FilterNode) -> FilterNode:
    """Negate a filter node."""
    return {"type": "not", "condition": _ensure_filter_node(node)}


def _ensure_filter_node(node: FilterNode) -> FilterNode:
    if not isinstance(node, MutableMapping) or "type" not in node:
        raise TypeError("Filter node must be a mapping with a 'type' key")
    return node


def build_select_ir(
    *,
    table: str,
    columns: Sequence[str] | None = None,
    col_types: dict[str, str] | None = None,
    model: str | None = None,
    column_mappings: dict[str, str] | None = None,
    filter_tree: FilterNode | None = None,
    distinct: bool | None = None,
    limit: int | None = None,
    offset: int | None = None,
    order_by: Sequence[tuple[str, str]] | None = None,
    joins: Sequence[dict[str, Any]] | None = None,
    group_by: Sequence[str] | None = None,
    having: FilterNode | None = None,
    aggregates: Sequence[dict[str, Any]] | None = None,
    returning: bool | None = None,
    exists: bool | None = None,
    count: bool | None = None,
    lock: str | None = None,
    pk_column: str | None = None,
) -> dict[str, Any]:
    """Build a SELECT query IR payload.

    Args:
        col_types: Optional mapping of column name to IR type hint.
            Used by Rust for type-aware decoding without type_info() calls.
            Types: "int", "str", "float", "bool", "bytes", "datetime", "date", "time"
        pk_column: Primary key column name, used for deduplication in JOIN queries.
    """
    payload: dict[str, Any] = {
        "proto": IR_PROTO_VERSION,
        "op": "select",
        "table": table,
    }
    # cols is optional when count=True or exists=True
    if columns:
        payload["cols"] = list(columns)
    if col_types:
        payload["col_types"] = dict(col_types)
    if filter_tree:
        payload["filter_tree"] = filter_tree
    if model:
        payload["model"] = model
    if column_mappings:
        payload["column_mappings"] = dict(column_mappings)
    if distinct is not None:
        payload["distinct"] = bool(distinct)
    if limit is not None:
        payload["limit"] = int(limit)
    if offset is not None:
        payload["offset"] = int(offset)
    if order_by:
        payload["order_by"] = [(str(col), direction) for col, direction in order_by]
    if joins:
        payload["joins"] = list(joins)
    if group_by:
        payload["group_by"] = list(group_by)
    if having:
        payload["having"] = having
    if aggregates:
        payload["aggregates"] = [dict(agg) for agg in aggregates]
    if returning is not None:
        payload["returning"] = bool(returning)
    if exists is not None:
        payload["exists"] = bool(exists)
    if count is not None:
        payload["count"] = bool(count)
    if lock is not None:
        payload["lock"] = lock
    if pk_column is not None:
        payload["pk_column"] = pk_column
    return payload


def build_insert_ir(
    *,
    table: str,
    values: dict[str, Any] | None = None,
    bulk_values: Sequence[dict[str, Any]] | None = None,
    col_types: dict[str, str] | None = None,
    model: str | None = None,
    returning: bool | None = None,
    pk_column: str | None = None,
) -> dict[str, Any]:
    """Build an INSERT query IR payload.

    Args:
        table: Table name
        values: Single row values dict
        bulk_values: Multiple rows for bulk insert
        col_types: Column type hints for proper parameter binding.
            Maps column name to IR type: "datetime", "date", "time", "uuid", etc.
        model: Model name for validation
        returning: Whether to return inserted rows
        pk_column: Primary key column name (defaults to "id" if not specified)
    """
    if not values and not bulk_values:
        raise ValueError("INSERT requires either 'values' or 'bulk_values'")
    payload: dict[str, Any] = {
        "proto": IR_PROTO_VERSION,
        "op": "insert",
        "table": table,
    }
    if values:
        payload["values"] = dict(values)
    if bulk_values:
        payload["bulk_values"] = [dict(row) for row in bulk_values]
    if col_types:
        payload["col_types"] = dict(col_types)
    if model:
        payload["model"] = model
    if returning is not None:
        payload["returning"] = bool(returning)
    if pk_column is not None:
        payload["pk_column"] = pk_column
    return payload


def build_update_ir(
    *,
    table: str,
    values: dict[str, Any] | None = None,
    filter_tree: FilterNode | None = None,
    col_types: dict[str, str] | None = None,
    model: str | None = None,
    returning: bool | None = None,
    bulk_update: Sequence[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    """Build an UPDATE query IR payload.

    Args:
        table: Table name
        values: Values to update
        filter_tree: Filter conditions
        col_types: Column type hints for proper parameter binding.
            Maps column name to IR type: "datetime", "date", "time", "uuid", etc.
        model: Model name
        returning: Whether to return updated rows
        bulk_update: Bulk update rows
    """
    if not values and not bulk_update:
        raise ValueError("UPDATE requires either 'values' or 'bulk_update'")
    if values and bulk_update:
        raise ValueError("Provide either 'values' or 'bulk_update', not both")
    payload: dict[str, Any] = {
        "proto": IR_PROTO_VERSION,
        "op": "update",
        "table": table,
    }
    if values:
        if not values:
            raise ValueError("UPDATE requires at least one value")
        payload["values"] = dict(values)
    if bulk_update:
        normalized_rows: list[dict[str, Any]] = []
        for entry in bulk_update:
            filters_payload = dict(entry.get("filters") or {})
            values_payload = dict(entry.get("values") or {})
            if not filters_payload or not values_payload:
                raise ValueError(
                    "Each bulk_update entry requires 'filters' and 'values'"
                )
            normalized_rows.append(
                {
                    "filters": filters_payload,
                    "values": values_payload,
                }
            )
        # Wrap in object with "rows" field to match Rust BulkUpdate struct
        payload["bulk_update"] = {"rows": normalized_rows}
    if col_types:
        payload["col_types"] = dict(col_types)
    if filter_tree:
        payload["filter_tree"] = filter_tree
    if model:
        payload["model"] = model
    if returning is not None:
        payload["returning"] = bool(returning)
    return payload


def build_delete_ir(
    *,
    table: str,
    filter_tree: FilterNode | None = None,
    model: str | None = None,
    returning: bool | None = None,
) -> dict[str, Any]:
    """Build a DELETE query IR payload."""
    payload: dict[str, Any] = {
        "proto": IR_PROTO_VERSION,
        "op": "delete",
        "table": table,
    }
    if filter_tree:
        payload["filter_tree"] = filter_tree
    if model:
        payload["model"] = model
    if returning is not None:
        payload["returning"] = bool(returning)
    return payload


def build_raw_sql_ir(*, sql: str, params: list[Any] | None = None) -> dict[str, Any]:
    """Build a raw SQL query IR payload.

    Args:
        sql: Raw SQL statement
        params: Optional parameters for the SQL statement

    Returns:
        IR payload dict
    """
    payload: dict[str, Any] = {
        "proto": IR_PROTO_VERSION,
        "op": "raw",
        "table": "",  # Empty table for raw SQL (required by QueryIR)
        "sql": sql,
    }
    if params:
        payload["params"] = list(params)
    return payload


__all__ = [
    "IR_PROTO_VERSION",
    "FilterNode",
    "filter_condition",
    "filter_and",
    "filter_or",
    "filter_not",
    "build_select_ir",
    "build_raw_sql_ir",
    "build_insert_ir",
    "build_update_ir",
    "build_delete_ir",
]

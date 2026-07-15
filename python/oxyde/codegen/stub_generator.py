"""Generator for .pyi stub files with type hints for model Queries."""

from __future__ import annotations

import ast
import copy
import inspect
import re
import sys
from datetime import date, datetime, time
from decimal import Decimal
from pathlib import Path
from typing import Any
from uuid import UUID

from oxyde.models.base import Model
from oxyde.models.lookups import (
    COMMON_LOOKUPS,
    DATE_PART_LOOKUPS,
    DATETIME_LOOKUPS,
    NUMERIC_LOOKUPS,
    STRING_LOOKUPS,
)
from oxyde.models.registry import registered_tables


def _get_python_type_name(python_type: Any) -> str:
    """Get string representation of Python type for stub file."""
    if python_type is str:
        return "str"
    elif python_type is int:
        return "int"
    elif python_type is float:
        return "float"
    elif python_type is bool:
        return "bool"
    elif python_type is bytes:
        return "bytes"
    elif python_type is datetime:
        return "datetime"
    elif python_type is date:
        return "date"
    elif python_type is time:
        return "time"
    elif python_type is Decimal:
        return "Decimal"
    elif python_type is UUID:
        return "UUID"
    elif isinstance(python_type, type) and issubclass(python_type, Model):
        # FK field pointing to another model
        return python_type.__name__
    elif hasattr(python_type, "__origin__"):
        # Parameterized generics (e.g. list[str], dict[str, Any])
        return str(python_type).replace("typing.", "")
    elif isinstance(python_type, type):
        # Bare built-in types (dict, list, set, tuple, etc.)
        return python_type.__name__
    else:
        # Other complex types (e.g. typing constructs)
        return str(python_type).replace("typing.", "")


def _get_lookups_for_type(python_type: Any) -> list[str]:
    """Get available lookups for a given Python type."""
    lookups = []

    # String types
    if python_type is str:
        lookups.extend(STRING_LOOKUPS)

    # Numeric types
    elif python_type in (int, float, Decimal):
        lookups.extend(NUMERIC_LOOKUPS)

    # Boolean types (no special lookups beyond common)
    elif python_type is bool:
        pass

    # Date/datetime types
    elif python_type in (datetime, date):
        lookups.extend(DATETIME_LOOKUPS)
        if python_type is datetime:
            lookups.extend(DATE_PART_LOOKUPS)

    # Add common lookups for all types
    lookups.extend(COMMON_LOOKUPS)

    return lookups


def _get_field_info(model_class: type[Model]) -> dict[str, tuple[Any, bool]]:
    """
    Get field info from model_fields, returns dict of field_name -> (python_type, is_pk).

    Uses model_fields (Pydantic) as primary source, with _db_meta as fallback for PK info.
    Excludes virtual fields (reverse FK, m2m) that don't map to DB columns.
    """
    from oxyde.models.field import OxydeFieldInfo
    from oxyde.models.utils import _unpack_annotated, _unwrap_optional

    result = {}

    for field_name, field_info in model_class.model_fields.items():
        # Skip virtual fields (reverse FK, m2m) - they don't map to DB columns
        if isinstance(field_info, OxydeFieldInfo):
            if getattr(field_info, "db_reverse_fk", None) or getattr(
                field_info, "db_m2m", False
            ):
                continue

        # Get python type from annotation
        annotation = field_info.annotation
        base_hint, _ = _unpack_annotated(annotation)
        python_type, _ = _unwrap_optional(base_hint)

        # Check if primary key
        is_pk = False
        if isinstance(field_info, OxydeFieldInfo):
            is_pk = field_info.db_pk or False
        else:
            json_extra = getattr(field_info, "json_schema_extra", None)
            if isinstance(json_extra, dict):
                is_pk = json_extra.get("db_pk", False)

        result[field_name] = (python_type, is_pk)

    return result


def _generate_filter_params(model_class: type[Model]) -> str:
    """Generate filter method parameters with all lookups."""
    lines = []
    field_info = _get_field_info(model_class)

    for field_name, (python_type, _is_pk) in sorted(field_info.items()):
        type_name = _get_python_type_name(python_type)

        # Exact match (field without lookup)
        lines.append(f"        {field_name}: {type_name} | None = None,")

        # Add all lookups for this type
        lookups = _get_lookups_for_type(python_type)
        for lookup in lookups:
            lookup_field = f"{field_name}__{lookup}"

            # Determine type for lookup
            if lookup == "in":
                lookup_type = f"list[{type_name}] | None"
            elif lookup in ("between", "range"):
                lookup_type = f"tuple[{type_name}, {type_name}] | None"
            elif lookup == "isnull":
                lookup_type = "bool | None"
            elif lookup in DATE_PART_LOOKUPS:
                lookup_type = "int | None"
            else:
                lookup_type = f"{type_name} | None"

            lines.append(f"        {lookup_field}: {lookup_type} = None,")

    return "\n".join(lines)


def _generate_order_by_literal(model_class: type[Model]) -> str:
    """Generate Literal type for order_by fields (includes - prefix for DESC)."""
    field_info = _get_field_info(model_class)
    literals = []

    for field_name in sorted(field_info.keys()):
        literals.append(f'"{field_name}"')
        literals.append(f'"-{field_name}"')

    if not literals:
        return "str"  # Fallback if no fields

    return f"Literal[{', '.join(literals)}]"


def _generate_field_literal(model_class: type[Model]) -> str:
    """Generate Literal type for field names (for select/values/group_by)."""
    field_info = _get_field_info(model_class)
    literals = [f'"{field_name}"' for field_name in sorted(field_info.keys())]

    if not literals:
        return "str"  # Fallback if no fields

    return f"Literal[{', '.join(literals)}]"


def _generate_update_params(model_class: type[Model]) -> str:
    """Generate update method parameters (field: type | None = None for each field)."""
    lines = []
    field_info = _get_field_info(model_class)

    for field_name, (python_type, is_pk) in sorted(field_info.items()):
        # Skip primary key - usually not updated
        if is_pk:
            continue

        type_name = _get_python_type_name(python_type)
        # All update params are optional
        lines.append(f"        {field_name}: {type_name} | None = None,")

    return "\n".join(lines)


def _generate_create_params(model_class: type[Model]) -> str:
    """Generate create method parameters with proper optionality."""
    lines = []
    field_info = _get_field_info(model_class)

    for field_name, (python_type, _is_pk) in sorted(field_info.items()):
        type_name = _get_python_type_name(python_type)
        # All create params are optional in stub (instance can be passed instead)
        lines.append(f"        {field_name}: {type_name} | None = None,")

    return "\n".join(lines)


def _has_overload_decorator(node: ast.FunctionDef | ast.AsyncFunctionDef) -> bool:
    """Return True if the function is decorated with ``@overload``."""
    for dec in node.decorator_list:
        if isinstance(dec, ast.Name) and dec.id == "overload":
            return True
        if isinstance(dec, ast.Attribute) and dec.attr == "overload":
            return True
    return False


def _extract_top_level_copyable(
    tree: ast.Module,
) -> tuple[list[ast.Import | ast.ImportFrom], list[str]]:
    """Collect top-level imports (as AST nodes) and function stubs from source.

    - ``Import`` / ``ImportFrom``: returned as AST nodes so the caller can
      dedupe them against the stub skeleton imports and drop unused ones.
      ``from __future__ import ...`` is dropped entirely — a ``.pyi`` never
      needs it, and anywhere but the top of the file it is a syntax error.
    - ``FunctionDef`` / ``AsyncFunctionDef``: returned with body replaced by
      ``...`` (stub convention). Decorators and type annotations are preserved.
      For ``@overload`` functions, only the ``@overload``-decorated variants are
      kept; the implementation body (PEP 484 requires one in a ``.py`` file but
      forbids it in a ``.pyi``) is dropped.
    - Everything else (top-level classes, dataclasses, enums, assignments) is
      intentionally skipped — those belong in dedicated modules.
    """
    overloaded_names: set[str] = {
        node.name
        for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
        and _has_overload_decorator(node)
    }

    imports: list[ast.Import | ast.ImportFrom] = []
    functions: list[str] = []
    for node in tree.body:
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            if isinstance(node, ast.ImportFrom) and node.module == "__future__":
                continue
            imports.append(node)
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            # Skip the non-@overload implementation of an overloaded function —
            # `.pyi` cannot contain such an implementation.
            if node.name in overloaded_names and not _has_overload_decorator(node):
                continue
            node_copy = copy.deepcopy(node)
            node_copy.body = [ast.Expr(value=ast.Constant(value=Ellipsis))]
            functions.append(ast.unparse(node_copy))
    return imports, functions


# Names the generated stub skeleton may reference, grouped by stdlib module.
# Each is emitted only when the generated body actually uses it.
_STUB_HEADER_IMPORTS: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("datetime", ("date", "datetime", "time")),
    ("decimal", ("Decimal",)),
    ("typing", ("Any", "ClassVar", "Literal")),
    ("uuid", ("UUID",)),
)


def _is_name_used(name: str, body: str) -> bool:
    return re.search(rf"\b{re.escape(name)}\b", body) is not None


def _assemble_imports(
    user_imports: list[ast.Import | ast.ImportFrom], body: str
) -> list[str]:
    """Build the stub import block: skeleton imports plus surviving user imports.

    Skeleton names (typing/datetime/decimal/uuid + oxyde) are emitted only when
    the generated body references them. User imports are kept for names the body
    still needs (cross-module FK types, custom-method signatures, decorators);
    unused names and duplicates of skeleton names are dropped — on a name
    collision the skeleton import wins, since the generated code references the
    skeleton's meaning of that name. Stdlib ``from`` imports are merged with the
    skeleton block so each module is imported once.
    """
    stdlib_from: dict[str, set[str]] = {}
    for module, names in _STUB_HEADER_IMPORTS:
        used = {name for name in names if _is_name_used(name, body)}
        if used:
            stdlib_from[module] = used

    provided: set[str] = {"Model", "Query", "QueryManager"}
    provided.update(*stdlib_from.values())

    other_lines: list[str] = []
    for node in user_imports:
        kept = [
            alias
            for alias in node.names
            if (bound := alias.asname or alias.name.split(".")[0]) not in provided
            and _is_name_used(bound, body)
        ]
        if not kept:
            continue
        if (
            isinstance(node, ast.ImportFrom)
            and node.level == 0
            and node.module is not None
            and node.module.split(".")[0] in sys.stdlib_module_names
            and all(alias.asname is None for alias in kept)
        ):
            stdlib_from.setdefault(node.module, set()).update(a.name for a in kept)
            continue
        node_copy = copy.deepcopy(node)
        node_copy.names = [copy.deepcopy(alias) for alias in kept]
        other_lines.append(ast.unparse(node_copy))

    lines = [
        f"from {module} import {', '.join(sorted(names))}"
        for module, names in sorted(stdlib_from.items())
    ]
    if lines:
        lines.append("")
    lines.extend(
        sorted(
            [
                "from oxyde import Model",
                "from oxyde.queries import Query, QueryManager",
                *other_lines,
            ]
        )
    )
    return lines


def _extract_custom_methods(class_def: ast.ClassDef) -> list[str]:
    """Extract user-defined methods from a Model ClassDef with body replaced by `...`.

    Returns list of method definitions as strings, each indented relative to the
    class body (i.e. without the leading class indent — caller adds it).
    """
    methods: list[str] = []
    for child in class_def.body:
        if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)):
            child_copy = copy.deepcopy(child)
            child_copy.body = [ast.Expr(value=ast.Constant(value=Ellipsis))]
            methods.append(ast.unparse(child_copy))
    return methods


def _generate_model_class_stub(
    model_class: type[Model],
    custom_methods: list[str],
) -> str:
    """Generate stub class definition for model (to avoid circular imports in .pyi)."""
    from oxyde.models.field import OxydeFieldInfo
    from oxyde.models.utils import _unpack_annotated, _unwrap_optional

    model_name = model_class.__name__
    lines = [f"class {model_name}(Model):"]

    # Add Meta class if present
    if hasattr(model_class, "Meta"):
        lines.append("    class Meta:")
        meta = model_class.Meta
        if hasattr(meta, "is_table"):
            lines.append("        is_table: bool")
        if hasattr(meta, "table_name"):
            lines.append("        table_name: str")
        if hasattr(meta, "schema"):
            lines.append("        schema: str")
        if hasattr(meta, "database"):
            lines.append("        database: str")

    # Add field annotations (exclude virtual fields like reverse FK, m2m)
    for field_name, field_info in model_class.model_fields.items():
        # Skip virtual fields (reverse FK, m2m) - they don't map to DB columns
        if isinstance(field_info, OxydeFieldInfo):
            if getattr(field_info, "db_reverse_fk", None) or getattr(
                field_info, "db_m2m", False
            ):
                continue

        annotation = field_info.annotation
        base_hint, _ = _unpack_annotated(annotation)
        python_type, is_optional = _unwrap_optional(base_hint)
        type_name = _get_python_type_name(python_type)

        if is_optional:
            lines.append(f"    {field_name}: {type_name} | None")
        else:
            lines.append(f"    {field_name}: {type_name}")

    # Copy user-defined methods (body replaced by `...`)
    for method_src in custom_methods:
        for raw_line in method_src.splitlines():
            lines.append(f"    {raw_line}" if raw_line else "")

    # Add objects manager with proper type. The base Model declares
    # `objects: ClassVar[QueryManager]`; narrowing it here is intentional,
    # so silence pyright's invariant-variable-override check.
    lines.append(
        f"    objects: ClassVar[{model_name}Manager]"
        "  # pyright: ignore[reportIncompatibleVariableOverride]"
    )

    return "\n".join(lines)


def generate_model_stub(model_class: type[Model]) -> str:
    """Generate .pyi stub content for a single model (without imports)."""
    model_name = model_class.__name__

    # Generate model-dependent parameters
    filter_params = _generate_filter_params(model_class)
    order_by_literal = _generate_order_by_literal(model_class)
    field_literal = _generate_field_literal(model_class)
    create_params = _generate_create_params(model_class)

    queryset_class = f"""
class {model_name}Query(Query[{model_name}]):
    \"\"\"Type-safe Query for {model_name} model.\"\"\"

    # Query building methods (sync, return Query)

    def filter(
        self,
        *args: Any,
{filter_params}
    ) -> {model_name}Query:
        \"\"\"Filter by Q-expressions or field lookups.\"\"\"
        ...

    def exclude(
        self,
        *args: Any,
{filter_params}
    ) -> {model_name}Query:
        \"\"\"Exclude objects matching field lookups.\"\"\"
        ...

    def order_by(self, *fields: {order_by_literal}) -> {model_name}Query:  # type: ignore[override]  # ty: ignore[invalid-method-override]
        \"\"\"Order results by fields.\"\"\"
        ...

    def limit(self, value: int) -> {model_name}Query:
        \"\"\"Limit number of results.\"\"\"
        ...

    def offset(self, value: int) -> {model_name}Query:
        \"\"\"Skip first value results.\"\"\"
        ...

    def distinct(self, distinct: bool = True) -> {model_name}Query:
        \"\"\"Return distinct results.\"\"\"
        ...

    def select(self, *fields: {field_literal}) -> {model_name}Query:  # type: ignore[override]  # ty: ignore[invalid-method-override]
        \"\"\"Select specific fields.\"\"\"
        ...

    def join(self, *paths: str) -> {model_name}Query:
        \"\"\"Perform LEFT JOIN for relations.\"\"\"
        ...

    def prefetch(self, *paths: str) -> {model_name}Query:
        \"\"\"Prefetch related objects (separate queries).\"\"\"
        ...

    def for_update(self) -> {model_name}Query:
        \"\"\"Add FOR UPDATE lock to query.\"\"\"
        ...

    def for_share(self) -> {model_name}Query:
        \"\"\"Add FOR SHARE lock to query.\"\"\"
        ...

    def annotate(self, **annotations: Any) -> {model_name}Query:
        \"\"\"Add computed fields using aggregate functions.\"\"\"
        ...

    def group_by(self, *fields: {field_literal}) -> {model_name}Query:  # type: ignore[override]  # ty: ignore[invalid-method-override]
        \"\"\"Add GROUP BY clause.\"\"\"
        ...

    def having(self, *q_exprs: Any, **kwargs: Any) -> {model_name}Query:
        \"\"\"Add HAVING clause for filtering grouped results.\"\"\"
        ...

    def values(self, *fields: {field_literal}) -> {model_name}Query:  # type: ignore[override]  # ty: ignore[invalid-method-override]
        \"\"\"Return dicts instead of models.\"\"\"
        ...

    def values_list(self, *fields: {field_literal}, flat: bool = False) -> {model_name}Query:  # type: ignore[override]  # ty: ignore[invalid-method-override]
        \"\"\"Return tuples/values instead of models.\"\"\"
        ...

    # Terminal methods (async, execute query)

    async def all(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> list[{model_name}]:
        \"\"\"Execute query and return all results.\"\"\"
        ...

    async def first(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> {model_name} | None:
        \"\"\"Execute query and return first result.\"\"\"
        ...

    async def last(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> {model_name} | None:
        \"\"\"Execute query and return last result.\"\"\"
        ...

    async def count(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> int:
        \"\"\"Count matching objects.\"\"\"
        ...

    async def sum(
        self,
        field: str,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> Any:
        \"\"\"Calculate sum of field values.\"\"\"
        ...

    async def avg(
        self,
        field: str,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> Any:
        \"\"\"Calculate average of field values.\"\"\"
        ...

    async def max(
        self,
        field: str,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> Any:
        \"\"\"Get maximum field value.\"\"\"
        ...

    async def min(
        self,
        field: str,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> Any:
        \"\"\"Get minimum field value.\"\"\"
        ...

    async def update(  # type: ignore[override]  # ty: ignore[invalid-method-override]
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
        **values: Any,
    ) -> int:
        \"\"\"Update matching objects.\"\"\"
        ...

    async def increment(
        self,
        field: str,
        by: int | float = 1,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> int:
        \"\"\"Atomically increment a field value.\"\"\"
        ...

    async def delete(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> int:
        \"\"\"Delete matching objects.\"\"\"
        ...
"""

    # Generate Manager class
    manager_class = f"""
class {model_name}Manager(QueryManager[{model_name}]):
    \"\"\"Type-safe Manager for {model_name} model.\"\"\"

    # Query building methods (sync, return Query)

    def query(self) -> {model_name}Query:
        \"\"\"Return a Query builder for this model.\"\"\"
        ...

    def filter(
        self,
        *args: Any,
{filter_params}
    ) -> {model_name}Query:
        \"\"\"Filter by Q-expressions or field lookups.\"\"\"
        ...

    def exclude(
        self,
        *args: Any,
{filter_params}
    ) -> {model_name}Query:
        \"\"\"Exclude objects matching field lookups.\"\"\"
        ...

    def values(self, *fields: {field_literal}) -> {model_name}Query:  # type: ignore[override]  # ty: ignore[invalid-method-override]
        \"\"\"Return dicts instead of models.\"\"\"
        ...

    def values_list(self, *fields: {field_literal}, flat: bool = False) -> {model_name}Query:  # type: ignore[override]  # ty: ignore[invalid-method-override]
        \"\"\"Return tuples/values instead of models.\"\"\"
        ...

    def distinct(self, distinct: bool = True) -> {model_name}Query:
        \"\"\"Return distinct results.\"\"\"
        ...

    def join(self, *paths: str) -> {model_name}Query:
        \"\"\"Perform LEFT JOIN for relations.\"\"\"
        ...

    def prefetch(self, *paths: str) -> {model_name}Query:
        \"\"\"Prefetch related objects (separate queries).\"\"\"
        ...

    def for_update(self) -> {model_name}Query:
        \"\"\"Add FOR UPDATE lock to query.\"\"\"
        ...

    def for_share(self) -> {model_name}Query:
        \"\"\"Add FOR SHARE lock to query.\"\"\"
        ...

    # Terminal methods (async, execute query)

    async def get(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
        **filters: Any,
    ) -> {model_name}:
        \"\"\"Get single object matching lookups.\"\"\"
        ...

    async def get_or_none(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
        **filters: Any,
    ) -> {model_name} | None:
        \"\"\"Get object or None if not found.\"\"\"
        ...

    async def get_or_create(
        self,
        *,
        defaults: dict[str, Any] | None = None,
        client: Any | None = None,
        using: str | None = None,
        **filters: Any,
    ) -> tuple[{model_name}, bool]:
        \"\"\"Get object or create if not found. Returns (object, created).\"\"\"
        ...

    async def update_or_create(
        self,
        *,
        defaults: dict[str, Any] | None = None,
        client: Any | None = None,
        using: str | None = None,
        **filters: Any,
    ) -> tuple[{model_name}, bool]:
        \"\"\"Get object, create if missing, or update it when defaults are provided.\"\"\"
        ...

    async def all(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
        mode: str = "models",
    ) -> list[{model_name}]:
        \"\"\"Get all objects.\"\"\"
        ...

    async def first(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> {model_name} | None:
        \"\"\"Get first object.\"\"\"
        ...

    async def last(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> {model_name} | None:
        \"\"\"Get last object.\"\"\"
        ...

    async def count(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> int:
        \"\"\"Count all objects.\"\"\"
        ...

    async def sum(
        self,
        field: str,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> Any:
        \"\"\"Calculate sum of field values.\"\"\"
        ...

    async def avg(
        self,
        field: str,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> Any:
        \"\"\"Calculate average of field values.\"\"\"
        ...

    async def max(
        self,
        field: str,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> Any:
        \"\"\"Get maximum field value.\"\"\"
        ...

    async def min(
        self,
        field: str,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> Any:
        \"\"\"Get minimum field value.\"\"\"
        ...

    async def create(  # type: ignore[override]  # ty: ignore[invalid-method-override]
        self,
        *,
        instance: {model_name} | None = None,
        client: Any | None = None,
        using: str | None = None,
{create_params}
    ) -> {model_name}:
        \"\"\"Create new object.\"\"\"
        ...

    async def bulk_create(  # type: ignore[override]  # ty: ignore[invalid-method-override]
        self,
        objects: list[{model_name}],
        *,
        batch_size: int | None = None,
        client: Any | None = None,
        using: str | None = None,
    ) -> list[{model_name}]:
        \"\"\"Bulk create objects.\"\"\"
        ...

    async def bulk_update(  # type: ignore[override]  # ty: ignore[invalid-method-override]
        self,
        objects: list[{model_name}],
        fields: list[str],
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> int:
        \"\"\"Bulk update objects.\"\"\"
        ...
"""

    return queryset_class + manager_class


def _build_stub(file_path: Path, models: list[type[Model]]) -> str:
    """Build .pyi stub content for a source file that contains Model classes.

    Policy: the .pyi covers models plus top-level functions (as stubs). From
    the source we also keep the imports still referenced by the generated body
    (cross-module FK types, custom-method signatures, decorators); everything
    else at top level (classes, dataclasses, constants) is intentionally not
    propagated — those belong in dedicated modules.
    """
    source = file_path.read_text()
    tree = ast.parse(source)

    user_imports, function_stubs = _extract_top_level_copyable(tree)

    model_names = {m.__name__ for m in models}
    custom_methods_by_model: dict[str, list[str]] = {}
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name in model_names:
            custom_methods_by_model[node.name] = _extract_custom_methods(node)

    body_parts: list[str] = []
    for function_stub in function_stubs:
        body_parts.append(function_stub)
        body_parts.append("")
    for model in models:
        body_parts.append(
            _generate_model_class_stub(
                model,
                custom_methods_by_model.get(model.__name__, []),
            )
        )
        body_parts.append("")
        body_parts.append(generate_model_stub(model))
        body_parts.append("")
    body = "\n".join(body_parts)

    parts: list[str] = [
        "# Auto-generated by oxyde generate-stubs",
        "# DO NOT EDIT - This file will be overwritten",
        "",
        *_assemble_imports(user_imports, body),
        "",
        body,
    ]
    return "\n".join(parts)


def generate_stub_for_file(file_path: Path) -> None:
    """Generate .pyi stub file for models in a Python file."""
    # Import the module to get model classes
    import importlib.util

    spec = importlib.util.spec_from_file_location("temp_module", file_path)
    if spec is None or spec.loader is None:
        print(f"Could not load module from {file_path}")
        return

    module = importlib.util.module_from_spec(spec)
    sys.modules["temp_module"] = module
    spec.loader.exec_module(module)

    # Find all Model subclasses in the module
    models = []
    for name in dir(module):
        obj = getattr(module, name)
        if (
            inspect.isclass(obj)
            and issubclass(obj, Model)
            and obj is not Model
            and getattr(obj, "_is_table", False)
        ):
            models.append(obj)

    if not models:
        print(f"No table models found in {file_path}")
        return

    stub_path = file_path.with_suffix(".pyi")
    stub_path.write_text(_build_stub(file_path, models))
    print(f"Generated stub: {stub_path}")


def generate_stubs_for_models(
    models: list[type[Model]] | None = None,
) -> dict[Path, str]:
    """
    Generate stubs for models and return mapping of file paths to stub content.

    Args:
        models: List of model classes. If None, uses all registered tables.

    Returns:
        Dict mapping source file paths to stub content
    """
    if models is None:
        models = list(registered_tables().values())

    # Group models by source file
    file_models: dict[Path, list[type[Model]]] = {}
    for model in models:
        if not getattr(model, "_is_table", False):
            continue

        source_file = inspect.getfile(model)
        file_path = Path(source_file)

        if file_path not in file_models:
            file_models[file_path] = []
        file_models[file_path].append(model)

    return {
        file_path.with_suffix(".pyi"): _build_stub(file_path, file_model_list)
        for file_path, file_model_list in file_models.items()
    }


def write_stubs(stub_mapping: dict[Path, str]) -> None:
    """Write stub files to disk."""
    for stub_path, content in stub_mapping.items():
        stub_path.write_text(content)
        print(f"Generated stub: {stub_path}")


__all__ = [
    "generate_model_stub",
    "generate_stub_for_file",
    "generate_stubs_for_models",
    "write_stubs",
]

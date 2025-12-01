"""Generator for .pyi stub files with type hints for model QuerySets."""

from __future__ import annotations

import inspect
from datetime import date, datetime, time
from decimal import Decimal
from pathlib import Path
from typing import Any
from uuid import UUID

from oxyde.models.base import OxydeModel
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
    else:
        # For complex types, use string representation
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


def _get_field_info(model_class: type[OxydeModel]) -> dict[str, tuple[Any, bool]]:
    """
    Get field info from model_fields, returns dict of field_name -> (python_type, is_pk).

    Uses model_fields (Pydantic) as primary source, with _db_meta as fallback for PK info.
    """
    from oxyde.models.field import OxydeFieldInfo
    from oxyde.models.utils import _unpack_annotated, _unwrap_optional

    result = {}

    for field_name, field_info in model_class.model_fields.items():
        # Get python type from annotation
        annotation = field_info.annotation
        base_hint, _ = _unpack_annotated(annotation)
        python_type, _ = _unwrap_optional(base_hint)

        # Check if primary key
        is_pk = False
        if isinstance(field_info, OxydeFieldInfo):
            is_pk = field_info.db_pk or False
        elif hasattr(field_info, "json_schema_extra") and field_info.json_schema_extra:
            is_pk = field_info.json_schema_extra.get("db_pk", False)

        result[field_name] = (python_type, is_pk)

    return result


def _generate_filter_params(model_class: type[OxydeModel]) -> str:
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
            elif lookup == "between":
                lookup_type = f"tuple[{type_name}, {type_name}] | None"
            elif lookup == "isnull":
                lookup_type = "bool | None"
            elif lookup in DATE_PART_LOOKUPS:
                lookup_type = "int | None"
            else:
                lookup_type = f"{type_name} | None"

            lines.append(f"        {lookup_field}: {lookup_type} = None,")

    return "\n".join(lines)


def _generate_order_by_literal(model_class: type[OxydeModel]) -> str:
    """Generate Literal type for order_by fields (includes - prefix for DESC)."""
    field_info = _get_field_info(model_class)
    literals = []

    for field_name in sorted(field_info.keys()):
        literals.append(f'"{field_name}"')
        literals.append(f'"-{field_name}"')

    if not literals:
        return "str"  # Fallback if no fields

    return f"Literal[{', '.join(literals)}]"


def _generate_field_literal(model_class: type[OxydeModel]) -> str:
    """Generate Literal type for field names (for select/values/group_by)."""
    field_info = _get_field_info(model_class)
    literals = [f'"{field_name}"' for field_name in sorted(field_info.keys())]

    if not literals:
        return "str"  # Fallback if no fields

    return f"Literal[{', '.join(literals)}]"


def _generate_update_params(model_class: type[OxydeModel]) -> str:
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


def _generate_create_params(model_class: type[OxydeModel]) -> str:
    """Generate create method parameters with proper optionality."""
    lines = []
    field_info = _get_field_info(model_class)

    for field_name, (python_type, _is_pk) in sorted(field_info.items()):
        type_name = _get_python_type_name(python_type)
        # All create params are optional in stub (instance can be passed instead)
        lines.append(f"        {field_name}: {type_name} | None = None,")

    return "\n".join(lines)


def generate_model_stub(model_class: type[OxydeModel]) -> str:
    """Generate .pyi stub content for a single model (without imports)."""
    model_name = model_class.__name__

    # Generate model-dependent parameters
    filter_params = _generate_filter_params(model_class)
    order_by_literal = _generate_order_by_literal(model_class)
    field_literal = _generate_field_literal(model_class)
    update_params = _generate_update_params(model_class)
    create_params = _generate_create_params(model_class)

    queryset_class = f"""
class {model_name}QuerySet(QuerySet[{model_name}]):
    \"\"\"Type-safe QuerySet for {model_name} model.\"\"\"

    def filter(
        self,
        *,
{filter_params}
    ) -> "{model_name}QuerySet":
        \"\"\"Filter queryset by field lookups.\"\"\"
        ...

    def exclude(
        self,
        *,
{filter_params}
    ) -> "{model_name}QuerySet":
        \"\"\"Exclude objects matching field lookups.\"\"\"
        ...

    def get(
        self,
        *,
{filter_params}
    ) -> {model_name}:
        \"\"\"Get single object matching lookups.\"\"\"
        ...

    def get_or_none(
        self,
        *,
{filter_params}
    ) -> {model_name} | None:
        \"\"\"Get object or None if not found.\"\"\"
        ...

    def order_by(self, *fields: {order_by_literal}) -> "{model_name}QuerySet":
        \"\"\"Order results by fields.\"\"\"
        ...

    def limit(self, n: int) -> "{model_name}QuerySet":
        \"\"\"Limit number of results.\"\"\"
        ...

    def offset(self, n: int) -> "{model_name}QuerySet":
        \"\"\"Skip first n results.\"\"\"
        ...

    def distinct(self, value: bool = True) -> "{model_name}QuerySet":
        \"\"\"Return distinct results.\"\"\"
        ...

    def select(self, *fields: {field_literal}) -> "{model_name}QuerySet":
        \"\"\"Select specific fields.\"\"\"
        ...

    def join(self, *paths: str) -> "{model_name}QuerySet":
        \"\"\"Perform LEFT JOIN for relations.\"\"\"
        ...

    def prefetch(self, *paths: str) -> "{model_name}QuerySet":
        \"\"\"Prefetch related objects (separate queries).\"\"\"
        ...

    def for_update(self) -> "{model_name}QuerySet":
        \"\"\"Add FOR UPDATE lock to query.\"\"\"
        ...

    def for_share(self) -> "{model_name}QuerySet":
        \"\"\"Add FOR SHARE lock to query.\"\"\"
        ...

    def annotate(self, **annotations: Any) -> "{model_name}QuerySet":
        \"\"\"Add computed fields using aggregate functions.\"\"\"
        ...

    def group_by(self, *fields: {field_literal}) -> "{model_name}QuerySet":
        \"\"\"Add GROUP BY clause.\"\"\"
        ...

    def having(self, *q_exprs: Any, **kwargs: Any) -> "{model_name}QuerySet":
        \"\"\"Add HAVING clause for filtering grouped results.\"\"\"
        ...

    def values(self, *fields: {field_literal}) -> "{model_name}QuerySet":
        \"\"\"Return dicts instead of models.\"\"\"
        ...

    def values_list(self, *fields: {field_literal}, flat: bool = False) -> "{model_name}QuerySet":
        \"\"\"Return tuples/values instead of models.\"\"\"
        ...

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

    async def exists(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
    ) -> bool:
        \"\"\"Check if any objects match.\"\"\"
        ...

    async def update(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
{update_params}
    ) -> int:
        \"\"\"Update matching objects.\"\"\"
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

    def filter(
        self,
        *,
{filter_params}
    ) -> {model_name}QuerySet:
        \"\"\"Filter queryset by field lookups.\"\"\"
        ...

    def exclude(
        self,
        *,
{filter_params}
    ) -> {model_name}QuerySet:
        \"\"\"Exclude objects matching field lookups.\"\"\"
        ...

    def values(self, *fields: {field_literal}) -> {model_name}QuerySet:
        \"\"\"Return dicts instead of models.\"\"\"
        ...

    def values_list(self, *fields: {field_literal}, flat: bool = False) -> {model_name}QuerySet:
        \"\"\"Return tuples/values instead of models.\"\"\"
        ...

    def distinct(self, distinct: bool = True) -> {model_name}QuerySet:
        \"\"\"Return distinct results.\"\"\"
        ...

    def join(self, *paths: str) -> {model_name}QuerySet:
        \"\"\"Perform LEFT JOIN for relations.\"\"\"
        ...

    def prefetch(self, *paths: str) -> {model_name}QuerySet:
        \"\"\"Prefetch related objects (separate queries).\"\"\"
        ...

    def for_update(self) -> {model_name}QuerySet:
        \"\"\"Add FOR UPDATE lock to query.\"\"\"
        ...

    def for_share(self) -> {model_name}QuerySet:
        \"\"\"Add FOR SHARE lock to query.\"\"\"
        ...

    async def get(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
{filter_params}
    ) -> {model_name}:
        \"\"\"Get single object matching lookups.\"\"\"
        ...

    async def get_or_none(
        self,
        *,
        client: Any | None = None,
        using: str | None = None,
{filter_params}
    ) -> {model_name} | None:
        \"\"\"Get object or None if not found.\"\"\"
        ...

    async def get_or_create(
        self,
        *,
        defaults: dict[str, Any] | None = None,
        client: Any | None = None,
        using: str | None = None,
{filter_params}
    ) -> tuple[{model_name}, bool]:
        \"\"\"Get object or create if not found. Returns (object, created).\"\"\"
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

    async def create(
        self,
        *,
        instance: {model_name} | None = None,
        client: Any | None = None,
        using: str | None = None,
{create_params}
    ) -> {model_name}:
        \"\"\"Create new object.\"\"\"
        ...

    async def bulk_create(
        self,
        objects: list[{model_name}],
        *,
        batch_size: int | None = None,
        client: Any | None = None,
        using: str | None = None,
    ) -> list[{model_name}]:
        \"\"\"Bulk create objects.\"\"\"
        ...
"""

    return queryset_class + manager_class


def generate_stub_for_file(file_path: Path) -> None:
    """Generate .pyi stub file for models in a Python file."""
    # Import the module to get model classes
    import importlib.util
    import sys

    spec = importlib.util.spec_from_file_location("temp_module", file_path)
    if spec is None or spec.loader is None:
        print(f"Could not load module from {file_path}")
        return

    module = importlib.util.module_from_spec(spec)
    sys.modules["temp_module"] = module
    spec.loader.exec_module(module)

    # Find all OxydeModel subclasses in the module
    models = []
    for name in dir(module):
        obj = getattr(module, name)
        if (
            inspect.isclass(obj)
            and issubclass(obj, OxydeModel)
            and obj is not OxydeModel
            and getattr(obj, "_is_table", False)
        ):
            models.append(obj)

    if not models:
        print(f"No table models found in {file_path}")
        return

    # Generate stub content
    stub_content = ""
    for model in models:
        stub_content += generate_model_stub(model) + "\n\n"

    # Write .pyi file
    stub_path = file_path.with_suffix(".pyi")
    stub_path.write_text(stub_content)
    print(f"Generated stub: {stub_path}")


def generate_stubs_for_models(
    models: list[type[OxydeModel]] | None = None,
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
    file_models: dict[Path, list[type[OxydeModel]]] = {}
    for model in models:
        if not getattr(model, "_is_table", False):
            continue

        # Get source file path
        source_file = inspect.getfile(model)
        file_path = Path(source_file)

        if file_path not in file_models:
            file_models[file_path] = []
        file_models[file_path].append(model)

    # Generate stubs
    result = {}
    for file_path, file_model_list in file_models.items():
        stub_content_parts = []

        # Get unique module names for imports
        module_names = {m.__module__ for m in file_model_list}

        # Common imports
        imports = [
            "# Auto-generated by oxyde generate-stubs",
            "# DO NOT EDIT - This file will be overwritten",
            "",
            "from typing import Any, Literal",
            "from datetime import datetime, date, time",
            "from decimal import Decimal",
            "from uuid import UUID",
            "",
        ]

        # Import models from their module
        for module_name in sorted(module_names):
            module_model_names = [
                m.__name__ for m in file_model_list if m.__module__ == module_name
            ]
            imports.append(
                f"from {module_name} import {', '.join(sorted(module_model_names))}"
            )

        imports.extend(
            [
                "from oxyde.queries.queryset import QuerySet",
                "from oxyde.queries.manager import QueryManager",
                "",
            ]
        )

        stub_content_parts.append("\n".join(imports))

        # Generate stub for each model
        for model in file_model_list:
            stub_content_parts.append(generate_model_stub(model))

        stub_content = "\n\n".join(stub_content_parts)
        result[file_path.with_suffix(".pyi")] = stub_content

    return result


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

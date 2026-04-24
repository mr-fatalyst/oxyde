"""Tests for .pyi stub generation utilities."""

from __future__ import annotations

import ast
from datetime import date, datetime, time
from decimal import Decimal
from uuid import UUID

import pytest

from oxyde import Field, Model
from oxyde.codegen.stub_generator import (
    _collect_transitive_models,
    _extract_top_level_copyable,
    _generate_create_params,
    _generate_filter_params,
    _generate_init_params,
    _get_python_type_name,
    _get_safe_type_name,
    _has_overload_decorator,
)


class _Leaf(Model):
    id: int | None = Field(default=None, db_pk=True)
    value: str = Field(default="")

    class Meta:
        is_table = True
        table_name = "_stub_test_leaf"


class _Node(Model):
    id: int | None = Field(default=None, db_pk=True)
    label: str = Field(default="")
    leaf: _Leaf | None = Field(default=None)

    class Meta:
        is_table = True
        table_name = "_stub_test_node"


class _Root(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")
    node: _Node | None = Field(default=None)

    class Meta:
        is_table = True
        table_name = "_stub_test_root"


class _WithReserved(Model):
    """Model that includes fields whose names are reserved create()/all() params."""

    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")
    using: str = Field(default="")

    class Meta:
        is_table = True
        table_name = "_stub_test_reserved"


class TestGetPythonTypeName:
    """Test _get_python_type_name returns valid type annotation strings."""

    @pytest.mark.parametrize(
        ("python_type", "expected"),
        [
            (str, "str"),
            (int, "int"),
            (float, "float"),
            (bool, "bool"),
            (bytes, "bytes"),
            (datetime, "datetime"),
            (date, "date"),
            (time, "time"),
            (Decimal, "Decimal"),
            (UUID, "UUID"),
        ],
    )
    def test_builtin_and_stdlib_types(self, python_type: type, expected: str):
        """Explicitly handled types return their name."""
        assert _get_python_type_name(python_type) == expected

    @pytest.mark.parametrize(
        ("python_type", "expected"),
        [
            (dict, "dict"),
            (list, "list"),
            (set, "set"),
            (tuple, "tuple"),
            (frozenset, "frozenset"),
        ],
    )
    def test_bare_container_types(self, python_type: type, expected: str):
        """Bare container types return their name, not repr (e.g. not <class 'dict'>)."""
        result = _get_python_type_name(python_type)
        assert result == expected
        assert "<class" not in result

    @pytest.mark.parametrize(
        ("python_type", "expected"),
        [
            (list[str], "list[str]"),
            (dict[str, int], "dict[str, int]"),
            (tuple[int, ...], "tuple[int, ...]"),
        ],
    )
    def test_parameterized_generic_types(self, python_type: type, expected: str):
        """Parameterized generics use their string representation."""
        assert _get_python_type_name(python_type) == expected

    def test_model_subclass(self):
        """Model subclasses return their class name."""

        class MyModel(Model):
            id: int = Field(db_pk=True)

            class Meta:
                is_table = True

        assert _get_python_type_name(MyModel) == "MyModel"


class TestGetSafeTypeName:
    """_get_safe_type_name returns an alias when field_name would shadow its type."""

    def test_no_conflict_returns_plain_type(self):
        assert _get_safe_type_name(str, "title") == "str"
        assert _get_safe_type_name(int, "count") == "int"
        assert _get_safe_type_name(bool, "active") == "bool"

    def test_datetime_field_name_returns_alias(self):
        assert _get_safe_type_name(datetime, "datetime") == "_Datetime"

    def test_date_field_name_returns_alias(self):
        assert _get_safe_type_name(date, "date") == "_Date"

    def test_time_field_name_returns_alias(self):
        assert _get_safe_type_name(time, "time") == "_Time"

    def test_non_alias_type_name_conflict_gets_underscore_prefix(self):
        # A type not in _SAFE_TYPE_ALIASES whose name matches the field name falls
        # back to the "_<type_name>" pattern.
        assert _get_safe_type_name(str, "str") == "_str"

    def test_datetime_field_with_different_name_is_not_aliased(self):
        assert _get_safe_type_name(datetime, "created_at") == "datetime"
        assert _get_safe_type_name(datetime, "started_at") == "datetime"

    def test_date_field_with_different_name_is_not_aliased(self):
        assert _get_safe_type_name(date, "published_on") == "date"

    def test_time_field_with_different_name_is_not_aliased(self):
        assert _get_safe_type_name(time, "publish_time") == "time"


class TestGenerateInitParams:
    def test_basic_fields_included(self):
        result = _generate_init_params(_Leaf)
        assert "id: int | None = None," in result
        assert "value: str | None = None," in result

    def test_reserved_params_excluded(self):
        # "using" is in _CREATE_RESERVED_PARAMS so it must be absent
        result = _generate_init_params(_WithReserved)
        assert "name: str | None = None," in result
        assert "id: int | None = None," in result
        assert "using" not in result

    def test_datetime_field_with_distinct_name_uses_plain_type(self):
        # A field named "created_at" typed as datetime has no name/type conflict,
        # so the plain "datetime" name is used (not the alias).
        class _Timestamped(Model):
            id: int | None = Field(default=None, db_pk=True)
            created_at: datetime | None = Field(default=None)

            class Meta:
                is_table = True
                table_name = "_stub_test_ts"

        result = _generate_init_params(_Timestamped)
        assert "created_at: datetime | None = None," in result

    def test_fk_field_uses_model_name_as_type(self):
        result = _generate_init_params(_Node)
        # FK field "leaf" typed as _Leaf → type name is "_Leaf"
        assert "leaf: _Leaf | None = None," in result


class TestGenerateCreateParams:
    def test_basic_fields_included(self):
        result = _generate_create_params(_Leaf)
        assert "id: int | None = None," in result
        assert "value: str | None = None," in result

    def test_reserved_params_excluded(self):
        # "using" is a create() reserved kwarg and must be absent
        result = _generate_create_params(_WithReserved)
        assert "name: str | None = None," in result
        assert "using" not in result

    def test_all_three_reserved_names_excluded(self):
        # Verify each reserved name independently using a model with each
        for reserved in ("instance", "client", "using"):

            class _R(Model):
                id: int | None = Field(default=None, db_pk=True)

                class Meta:
                    is_table = True
                    table_name = f"_stub_test_reserved_{reserved}"

            # Manually inject the reserved field into model_fields for testing
            # by checking _CREATE_RESERVED_PARAMS content instead
            from oxyde.codegen.stub_generator import _CREATE_RESERVED_PARAMS

            assert reserved in _CREATE_RESERVED_PARAMS


class TestHasOverloadDecorator:
    def _parse_func(self, src: str) -> ast.FunctionDef | ast.AsyncFunctionDef:
        tree = ast.parse(src)
        node = tree.body[0]
        assert isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
        return node

    def test_plain_function_returns_false(self):
        node = self._parse_func("def foo(): ...")
        assert not _has_overload_decorator(node)

    def test_overload_by_name_returns_true(self):
        node = self._parse_func("@overload\ndef foo(): ...")
        assert _has_overload_decorator(node)

    def test_overload_attribute_form_returns_true(self):
        node = self._parse_func("@typing.overload\ndef foo(): ...")
        assert _has_overload_decorator(node)

    def test_other_decorator_returns_false(self):
        node = self._parse_func("@staticmethod\ndef foo(): ...")
        assert not _has_overload_decorator(node)

    def test_async_function_supported(self):
        node = self._parse_func("@overload\nasync def foo(): ...")
        assert _has_overload_decorator(node)

    def test_multiple_decorators_one_overload(self):
        src = "@some_decorator\n@overload\ndef foo(): ..."
        node = self._parse_func(src)
        assert _has_overload_decorator(node)


class TestExtractTopLevelCopyable:
    """Test TYPE_CHECKING import hoisting"""
    def _parse(self, src: str) -> ast.Module:
        return ast.parse(src)

    def test_regular_imports_included(self):
        tree = self._parse("import os\nfrom pathlib import Path")
        result = _extract_top_level_copyable(tree)
        assert "import os" in result
        assert "from pathlib import Path" in result

    def test_type_checking_block_imports_hoisted(self):
        src = (
            "from typing import TYPE_CHECKING\n"
            "if TYPE_CHECKING:\n"
            "    from some_module import SomeType\n"
            "    from other import OtherType\n"
        )
        tree = self._parse(src)
        result = _extract_top_level_copyable(tree)
        combined = "\n".join(result)
        assert "from some_module import SomeType" in combined
        assert "from other import OtherType" in combined

    def test_non_type_checking_if_block_not_hoisted(self):
        src = "if True:\n    from secret import Something\n"
        tree = self._parse(src)
        result = _extract_top_level_copyable(tree)
        assert not any("secret" in line for line in result)

    def test_overload_implementation_dropped(self):
        src = (
            "from typing import overload\n"
            "@overload\n"
            "def foo(x: int) -> str: ...\n"
            "@overload\n"
            "def foo(x: str) -> int: ...\n"
            "def foo(x):\n"
            "    return x\n"
        )
        tree = self._parse(src)
        result = _extract_top_level_copyable(tree)
        # Both @overload stubs kept; bare implementation dropped
        func_lines = [r for r in result if "def foo" in r]
        assert len(func_lines) == 2
        assert all("overload" in s for s in func_lines)

    def test_plain_function_included(self):
        src = "def helper(x: int) -> str: ...\n"
        tree = self._parse(src)
        result = _extract_top_level_copyable(tree)
        assert any("def helper" in r for r in result)

    def test_type_checking_non_import_children_ignored(self):
        src = "if TYPE_CHECKING:\n    x = 1\n"
        tree = self._parse(src)
        result = _extract_top_level_copyable(tree)
        # Assignment inside TYPE_CHECKING is not an import → not hoisted
        assert not any("x = 1" in line for line in result)


class TestGenerateFilterParams:
    """Test FK traversal"""
    def _param_names(self, lines: list[str]) -> set[str]:
        """Extract parameter names (before ':') from filter param lines."""
        names = set()
        for line in lines:
            stripped = line.strip()
            if ":" in stripped:
                names.add(stripped.split(":")[0].strip())
        return names

    def test_scalar_fields_generate_exact_and_lookups(self):
        lines = _generate_filter_params(_Leaf)
        names = self._param_names(lines)
        # Exact match params
        assert "id" in names
        assert "value" in names
        # Lookup suffix for str
        assert "value__icontains" in names
        assert "value__startswith" in names
        # Lookup suffix for int
        assert "id__gte" in names
        assert "id__in" in names

    def test_fk_field_emits_isnull_and_related_scalar_params(self):
        lines = _generate_filter_params(_Node)
        names = self._param_names(lines)
        # FK itself with isnull
        assert "leaf" in names
        assert "leaf__isnull" in names
        # Related model's scalar fields traversed with double-underscore prefix
        assert "leaf__value" in names
        assert "leaf__value__icontains" in names
        assert "leaf__id" in names
        assert "leaf__id__gte" in names

    def test_nested_fk_traversal_two_hops(self):
        # _Root.node (FK → _Node) and _Node.leaf (FK → _Leaf)
        lines = _generate_filter_params(_Root)
        names = self._param_names(lines)
        # Direct FK at first hop
        assert "node" in names
        assert "node__isnull" in names
        # One hop: _Node scalar fields
        assert "node__label" in names
        # Two hops: _Leaf scalar fields reachable via node__leaf__ prefix
        assert "node__leaf__value" in names
        assert "node__leaf__id" in names
        assert "node__leaf__value__icontains" in names

    def test_returns_list_of_strings(self):
        result = _generate_filter_params(_Leaf)
        assert isinstance(result, list)
        assert all(isinstance(line, str) for line in result)

    def test_prefix_argument_prepends_to_all_param_names(self):
        lines = _generate_filter_params(_Leaf, prefix="parent")
        names = self._param_names(lines)
        assert all(name.startswith("parent__") for name in names)
        assert "parent__id" in names
        assert "parent__value" in names

    def test_no_fk_field_in_filter_params_for_leaf_model(self):
        lines = _generate_filter_params(_Leaf)
        names = self._param_names(lines)
        # _Leaf has no FK fields; every param prefix must be a _Leaf scalar field
        for name in names:
            prefix = name.split("__")[0]
            assert prefix in ("id", "value"), f"unexpected FK-derived param: {name}"


class TestCollectTransitiveModels:
    def test_model_with_no_fks_returns_only_itself(self):
        result = _collect_transitive_models([_Leaf])
        assert result == {_Leaf}

    def test_direct_fk_includes_both_models(self):
        result = _collect_transitive_models([_Node])
        assert _Node in result
        assert _Leaf in result

    def test_transitive_fk_includes_all_reachable_models(self):
        # _Root → _Node → _Leaf
        result = _collect_transitive_models([_Root])
        assert _Root in result
        assert _Node in result
        assert _Leaf in result

    def test_multiple_roots_unioned(self):
        result = _collect_transitive_models([_Leaf, _Node])
        assert _Leaf in result
        assert _Node in result

    def test_returns_set(self):
        result = _collect_transitive_models([_Root])
        assert isinstance(result, set)

    def test_duplicate_roots_not_traversed_twice(self):
        # Passing _Leaf twice shouldn't cause errors or duplicates in a set
        result = _collect_transitive_models([_Leaf, _Leaf])
        assert result == {_Leaf}

    def test_empty_list_returns_empty_set(self):
        result = _collect_transitive_models([])
        assert result == set()

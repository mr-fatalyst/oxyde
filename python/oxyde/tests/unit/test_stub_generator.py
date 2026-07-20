"""Tests for .pyi stub generation utilities."""

from __future__ import annotations

import ast
from datetime import date, datetime, time
from decimal import Decimal
from uuid import UUID

import pytest

from oxyde import Field, Model
from oxyde.codegen.stub_generator import (
    _assemble_imports,
    _extract_custom_methods,
    _extract_top_level_copyable,
    _generate_create_params,
    _generate_filter_params,
    _get_python_type_name,
)


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


class TestExtractTopLevelCopyable:
    """Test extraction of imports and function stubs from model source."""

    def test_future_import_dropped(self):
        """`from __future__ import ...` must never reach the stub (issue #43)."""
        tree = ast.parse(
            "from __future__ import annotations\nfrom pydantic import field_validator\n"
        )
        imports, functions = _extract_top_level_copyable(tree)
        assert [ast.unparse(node) for node in imports] == [
            "from pydantic import field_validator"
        ]
        assert functions == []

    def test_overload_implementation_dropped(self):
        """Only @overload variants survive; the implementation body is dropped."""
        tree = ast.parse(
            "from typing import overload\n"
            "@overload\n"
            "def f(x: int) -> str: ...\n"
            "@overload\n"
            "def f(x: str) -> int: ...\n"
            "def f(x): return x\n"
        )
        _, functions = _extract_top_level_copyable(tree)
        assert len(functions) == 2
        assert all("@overload" in fn for fn in functions)


class TestAssembleImports:
    """Test the stub import block assembly (dedupe, unused-drop, merging)."""

    @staticmethod
    def _imports(source: str) -> list[ast.Import | ast.ImportFrom]:
        imports, _ = _extract_top_level_copyable(ast.parse(source))
        return imports

    def test_skeleton_names_deduped_and_unused_dropped(self):
        """User re-imports of skeleton names and unused names are dropped."""
        imports = self._imports(
            "from datetime import datetime\n"
            "from typing import Any\n"
            "from uuid import UUID, uuid4\n"
            "from oxyde import Field, Model\n"
        )
        body = "created_at: datetime\nmeta: dict[str, Any]\nslug: UUID\nx: Any\n"
        lines = _assemble_imports(imports, body)
        joined = "\n".join(lines)
        assert joined.count("from datetime import") == 1
        assert joined.count("from uuid import") == 1
        assert "uuid4" not in joined
        assert "Field" not in joined
        assert "from oxyde import Model" in lines

    def test_used_third_party_import_survives(self):
        """Imports the stub body still references are kept (cross-module FK)."""
        imports = self._imports(
            "from pydantic import model_validator\nfrom sibling import Owner\n"
        )
        body = "@model_validator(mode='after')\nowner: Owner | None\n"
        lines = _assemble_imports(imports, body)
        assert "from pydantic import model_validator" in lines
        assert "from sibling import Owner" in lines

    def test_stdlib_user_import_merged_into_skeleton_block(self):
        """A user stdlib from-import merges with the skeleton's module line."""
        imports = self._imports("from typing import overload\n")
        body = "@overload\ndef f(x: int) -> str: ...\nx: Any\ny: ClassVar[int]\n"
        lines = _assemble_imports(imports, body)
        typing_lines = [ln for ln in lines if ln.startswith("from typing import")]
        assert typing_lines == ["from typing import Any, ClassVar, overload"]


class TestGenerateFilterParams:
    """Test lookup parameter generation."""

    def test_range_lookup_takes_a_pair(self):
        """`__range`/`__between` accept a (lo, hi) pair as tuple or list."""

        class Ranged(Model):
            id: int = Field(db_pk=True)

        params = _generate_filter_params(Ranged)
        assert "id__range: tuple[int, int] | list[int] | None = None," in params
        assert "id__between: tuple[int, int] | list[int] | None = None," in params

    def test_in_lookup_takes_any_iterable(self):
        """`__in` accepts any iterable at runtime (tuple, set, generator)."""

        class Inny(Model):
            id: int = Field(db_pk=True)

        params = _generate_filter_params(Inny)
        assert "id__in: Iterable[int] | None = None," in params

    def test_date_part_lookups_match_runtime_shapes(self):
        """Runtime wants year=int, month=(y, m), day=(y, m, d)."""

        class Dated(Model):
            id: int = Field(db_pk=True)
            created: datetime = Field(default_factory=datetime.now)

        params = _generate_filter_params(Dated)
        assert "created__year: int | None = None," in params
        assert "created__month: tuple[int, int] | list[int] | None = None," in params
        assert (
            "created__day: tuple[int, int, int] | list[int] | None = None," in params
        )

    def test_reserved_field_names_not_enumerated(self):
        """Fields named like service params must not produce duplicate args."""

        class Clashy(Model):
            id: int = Field(db_pk=True)
            client: str = Field(default="")
            kwargs: str = Field(default="")
            defaults: str = Field(default="")

        params = _generate_filter_params(Clashy)
        for reserved in ("client", "kwargs", "defaults"):
            assert f"\n        {reserved}:" not in f"\n{params}"
        assert params.rstrip().endswith("**kwargs: Any,")
        # The resulting signature must stay syntactically valid
        ast.parse(f"def f(\n        self,\n        *args,\n{params}\n): ...\n")

        create_params = _generate_create_params(Clashy)
        assert "\n        client:" not in f"\n{create_params}"
        ast.parse(
            "def f(\n        self,\n        *,\n        instance=None,\n"
            "        client=None,\n        using=None,\n"
            f"{create_params}\n): ...\n"
        )


class TestExtractCustomMethods:
    """Test model-method stubbing."""

    def test_class_overload_implementation_dropped(self):
        """@overload variants survive; the in-class implementation is dropped."""
        tree = ast.parse(
            "class M:\n"
            "    @overload\n"
            "    def render(self, short: bool) -> str: ...\n"
            "    @overload\n"
            "    def render(self) -> bytes: ...\n"
            "    def render(self, short=None):\n"
            "        return b''\n"
            "    def plain(self) -> int:\n"
            "        return 1\n"
        )
        class_def = tree.body[0]
        assert isinstance(class_def, ast.ClassDef)
        methods = _extract_custom_methods(class_def)
        renders = [m for m in methods if "def render" in m]
        assert len(renders) == 2
        assert all("@overload" in m for m in renders)
        assert any("def plain" in m for m in methods)

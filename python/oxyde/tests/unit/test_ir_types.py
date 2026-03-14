"""Tests for IR type mapping, including array type support."""

from __future__ import annotations

from datetime import date, datetime, time, timedelta
from decimal import Decimal
from typing import Optional
from uuid import UUID

import pytest

from oxyde.core.ir_types import get_ir_type


class TestGetIrTypeScalar:
    """Scalar types should return their IR name."""

    @pytest.mark.parametrize(
        "python_type, expected",
        [
            (str, "str"),
            (int, "int"),
            (float, "float"),
            (bool, "bool"),
            (bytes, "bytes"),
            (datetime, "datetime"),
            (date, "date"),
            (time, "time"),
            (timedelta, "timedelta"),
            (UUID, "uuid"),
            (Decimal, "decimal"),
            (dict, "json"),
        ],
    )
    def test_scalar_types(self, python_type, expected):
        assert get_ir_type(python_type) == expected


class TestGetIrTypeOptional:
    """Optional[T] should return the same as T."""

    def test_optional_str(self):
        assert get_ir_type(Optional[str]) == "str"

    def test_optional_uuid(self):
        assert get_ir_type(Optional[UUID]) == "uuid"

    def test_optional_int(self):
        assert get_ir_type(Optional[int]) == "int"


class TestGetIrTypeArray:
    """list[T] types should return '{ir_type}[]'."""

    @pytest.mark.parametrize(
        "python_type, expected",
        [
            (list[str], "str[]"),
            (list[int], "int[]"),
            (list[float], "float[]"),
            (list[bool], "bool[]"),
            (list[UUID], "uuid[]"),
            (list[datetime], "datetime[]"),
            (list[date], "date[]"),
            (list[Decimal], "decimal[]"),
        ],
    )
    def test_list_types(self, python_type, expected):
        assert get_ir_type(python_type) == expected

    def test_optional_list_str(self):
        """Optional[list[str]] should return 'str[]'."""
        assert get_ir_type(Optional[list[str]]) == "str[]"

    def test_optional_list_uuid(self):
        """Optional[list[UUID]] should return 'uuid[]'."""
        assert get_ir_type(Optional[list[UUID]]) == "uuid[]"

    def test_union_list_none(self):
        """list[str] | None should return 'str[]'."""
        assert get_ir_type(list[str] | None) == "str[]"

    def test_bare_list_returns_none(self):
        """Bare list without element type should return None."""
        assert get_ir_type(list) is None


class TestGetIrTypeUnsupported:
    """Unsupported types should return None."""

    def test_custom_class(self):
        class Foo:
            pass

        assert get_ir_type(Foo) is None

    def test_set(self):
        assert get_ir_type(set) is None

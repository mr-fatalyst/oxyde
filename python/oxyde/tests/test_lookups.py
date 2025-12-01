"""Parameterized tests for all lookup types and field filtering."""

from __future__ import annotations

from datetime import date, datetime
from decimal import Decimal
from typing import Any

import pytest

from oxyde import Field, OxydeModel
from oxyde.exceptions import FieldError, LookupError, LookupValueError
from oxyde.models.lookups import (
    _allowed_lookups_for_meta,
    _lookup_category,
    _resolve_column_meta,
    _split_lookup_key,
)
from oxyde.models.registry import clear_registry, registered_tables


def get_filter_condition(ir: dict[str, Any]) -> dict[str, Any]:
    """Extract single filter condition from IR filter_tree."""
    tree = ir.get("filter_tree")
    if tree is None:
        raise KeyError("No filter_tree in IR")
    # Single condition
    if tree.get("type") == "condition":
        return tree
    # AND with conditions - return first
    raise ValueError(f"Expected single condition, got {tree.get('type')}")


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


class TestModel(OxydeModel):
    """Test model with various field types."""

    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str | None = None
    age: int
    price: float
    balance: Decimal = Decimal("0.00")
    is_active: bool = True
    created_at: datetime
    birth_date: date | None = None

    class Meta:
        is_table = True


class TestSplitLookupKey:
    """Test _split_lookup_key function."""

    def test_simple_field(self):
        """Test field name without lookup."""
        field, lookup = _split_lookup_key("name")
        assert field == "name"
        assert lookup == "exact"

    def test_field_with_lookup(self):
        """Test field name with lookup."""
        field, lookup = _split_lookup_key("name__icontains")
        assert field == "name"
        assert lookup == "icontains"

    def test_field_with_double_underscore(self):
        """Test field with multiple underscores."""
        field, lookup = _split_lookup_key("created_at__year")
        assert field == "created_at"
        assert lookup == "year"

    def test_empty_field_raises(self):
        """Test that empty field name raises error."""
        with pytest.raises(LookupError):
            _split_lookup_key("__exact")


class TestLookupCategory:
    """Test _lookup_category function."""

    def test_string_category(self):
        """Test string field category."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "name")
        assert _lookup_category(meta) == "string"

    def test_numeric_category_int(self):
        """Test integer field category."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "age")
        assert _lookup_category(meta) == "numeric"

    def test_numeric_category_float(self):
        """Test float field category."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "price")
        assert _lookup_category(meta) == "numeric"

    def test_numeric_category_decimal(self):
        """Test Decimal field category."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "balance")
        assert _lookup_category(meta) == "numeric"

    def test_datetime_category(self):
        """Test datetime field category."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "created_at")
        assert _lookup_category(meta) == "datetime"

    def test_date_category(self):
        """Test date field category."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "birth_date")
        assert _lookup_category(meta) == "datetime"

    def test_bool_category(self):
        """Test boolean field category."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "is_active")
        # Note: bool is treated as numeric in lookup system
        category = _lookup_category(meta)
        assert category in ("bool", "generic", "numeric")


class TestAllowedLookups:
    """Test _allowed_lookups_for_meta function."""

    def test_string_field_lookups(self):
        """Test allowed lookups for string fields."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "name")
        allowed = _allowed_lookups_for_meta(meta)

        assert "exact" in allowed
        assert "contains" in allowed
        assert "icontains" in allowed
        assert "startswith" in allowed
        assert "endswith" in allowed
        assert "iexact" in allowed
        assert "in" in allowed
        assert "isnull" in allowed

    def test_numeric_field_lookups(self):
        """Test allowed lookups for numeric fields."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "age")
        allowed = _allowed_lookups_for_meta(meta)

        assert "exact" in allowed
        assert "gt" in allowed
        assert "gte" in allowed
        assert "lt" in allowed
        assert "lte" in allowed
        assert "between" in allowed
        assert "in" in allowed
        assert "isnull" in allowed

    def test_datetime_field_lookups(self):
        """Test allowed lookups for datetime fields."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "created_at")
        allowed = _allowed_lookups_for_meta(meta)

        assert "exact" in allowed
        assert "gt" in allowed
        assert "gte" in allowed
        assert "lt" in allowed
        assert "lte" in allowed
        assert "between" in allowed
        assert "year" in allowed
        assert "month" in allowed
        assert "day" in allowed
        assert "in" in allowed
        assert "isnull" in allowed

    def test_bool_field_lookups(self):
        """Test allowed lookups for boolean fields."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "is_active")
        allowed = _allowed_lookups_for_meta(meta)

        assert "exact" in allowed
        assert "in" in allowed
        assert "isnull" in allowed


class TestStringLookups:
    """Test string-specific lookups."""

    @pytest.mark.parametrize(
        "lookup,operator,pattern_fn",
        [
            ("contains", "LIKE", lambda v: f"%{v}%"),
            ("icontains", "ILIKE", lambda v: f"%{v}%"),
            ("startswith", "LIKE", lambda v: f"{v}%"),
            ("istartswith", "ILIKE", lambda v: f"{v}%"),
            ("endswith", "LIKE", lambda v: f"%{v}"),
            ("iendswith", "ILIKE", lambda v: f"%{v}"),
        ],
    )
    def test_string_pattern_lookups(self, lookup, operator, pattern_fn):
        """Test string pattern lookups generate correct conditions."""
        registered_tables()
        ir = TestModel.objects.filter(**{f"name__{lookup}": "test"}).to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == operator
        assert cond["value"] == pattern_fn("test")

    def test_iexact_lookup(self):
        """Test iexact lookup."""
        registered_tables()
        ir = TestModel.objects.filter(name__iexact="Test").to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == "ILIKE"
        assert cond["value"] == "Test"

    def test_contains_escapes_wildcards(self):
        """Test that contains escapes SQL wildcards."""
        registered_tables()
        ir = TestModel.objects.filter(name__contains="test%value").to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == "LIKE"
        assert "\\%" in cond["value"]

    def test_contains_escapes_underscore(self):
        """Test that contains escapes underscore."""
        registered_tables()
        ir = TestModel.objects.filter(name__contains="test_value").to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == "LIKE"
        assert "\\_" in cond["value"]

    def test_string_lookup_requires_string_value(self):
        """Test that string lookups require string values."""
        registered_tables()

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(name__contains=123)

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(name__iexact=123)


class TestNumericLookups:
    """Test numeric-specific lookups."""

    @pytest.mark.parametrize(
        "lookup,operator",
        [
            ("gt", ">"),
            ("gte", ">="),
            ("lt", "<"),
            ("lte", "<="),
        ],
    )
    def test_comparison_lookups(self, lookup, operator):
        """Test comparison lookups generate correct conditions."""
        registered_tables()
        ir = TestModel.objects.filter(**{f"age__{lookup}": 18}).to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == operator
        assert cond["value"] == 18

    def test_between_lookup(self):
        """Test between lookup."""
        registered_tables()
        ir = TestModel.objects.filter(age__between=(18, 65)).to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == "BETWEEN"
        assert cond["value"] == [18, 65]

    def test_between_requires_tuple(self):
        """Test that between requires a tuple/list of 2 values."""
        registered_tables()

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(age__between=18)

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(age__between=(18,))

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(age__between=(18, 25, 30))

    def test_comparison_requires_non_null(self):
        """Test that comparison lookups require non-null values."""
        registered_tables()

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(age__gt=None)


class TestCommonLookups:
    """Test common lookups available for all field types."""

    def test_exact_lookup(self):
        """Test exact lookup."""
        registered_tables()
        ir = TestModel.objects.filter(name="Alice").to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == "="
        assert cond["value"] == "Alice"

    def test_exact_with_none(self):
        """Test exact lookup with None value."""
        registered_tables()
        ir = TestModel.objects.filter(email=None).to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == "IS NULL"

    def test_in_lookup(self):
        """Test in lookup."""
        registered_tables()
        ir = TestModel.objects.filter(age__in=[18, 21, 25]).to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == "IN"
        assert cond["value"] == [18, 21, 25]

    def test_in_requires_iterable(self):
        """Test that in lookup requires an iterable."""
        registered_tables()

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(age__in=18)

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(age__in=None)

    def test_in_rejects_string(self):
        """Test that in lookup rejects string (which is iterable but wrong)."""
        registered_tables()

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(name__in="test")

    def test_isnull_true(self):
        """Test isnull=True lookup."""
        registered_tables()
        ir = TestModel.objects.filter(email__isnull=True).to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == "IS NULL"

    def test_isnull_false(self):
        """Test isnull=False lookup."""
        registered_tables()
        ir = TestModel.objects.filter(email__isnull=False).to_ir()
        cond = get_filter_condition(ir)

        assert cond["operator"] == "IS NOT NULL"


def get_and_conditions(ir: dict[str, Any]) -> list[dict[str, Any]]:
    """Extract conditions from AND filter_tree."""
    tree = ir.get("filter_tree")
    if tree is None:
        raise KeyError("No filter_tree in IR")
    if tree.get("type") == "and":
        return tree.get("conditions", [])
    raise ValueError(f"Expected AND, got {tree.get('type')}")


class TestDatePartLookups:
    """Test date/datetime part lookups."""

    def test_year_lookup(self):
        """Test year lookup."""
        registered_tables()
        ir = TestModel.objects.filter(created_at__year=2024).to_ir()
        conditions = get_and_conditions(ir)

        # Year lookup generates two conditions: >= start AND < end
        assert len(conditions) == 2
        assert conditions[0]["operator"] == ">="
        assert conditions[1]["operator"] == "<"
        assert "2024-01-01" in conditions[0]["value"]
        assert "2025-01-01" in conditions[1]["value"]

    def test_month_lookup(self):
        """Test month lookup."""
        registered_tables()
        ir = TestModel.objects.filter(created_at__month=(2024, 3)).to_ir()
        conditions = get_and_conditions(ir)

        assert len(conditions) == 2
        assert "2024-03-01" in conditions[0]["value"]
        assert "2024-04-01" in conditions[1]["value"]

    def test_month_lookup_december_wraparound(self):
        """Test month lookup for December (wraps to next year)."""
        registered_tables()
        ir = TestModel.objects.filter(created_at__month=(2024, 12)).to_ir()
        conditions = get_and_conditions(ir)

        assert "2024-12-01" in conditions[0]["value"]
        assert "2025-01-01" in conditions[1]["value"]

    def test_day_lookup(self):
        """Test day lookup."""
        registered_tables()
        ir = TestModel.objects.filter(created_at__day=(2024, 3, 15)).to_ir()
        conditions = get_and_conditions(ir)

        assert len(conditions) == 2
        assert "2024-03-15" in conditions[0]["value"]
        assert "2024-03-16" in conditions[1]["value"]

    def test_month_requires_tuple(self):
        """Test that month lookup requires (year, month) tuple."""
        registered_tables()

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(created_at__month=3)

    def test_day_requires_tuple(self):
        """Test that day lookup requires (year, month, day) tuple."""
        registered_tables()

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(created_at__day=(2024, 3))

    def test_month_validates_range(self):
        """Test that month value is validated."""
        registered_tables()

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(created_at__month=(2024, 13))

        with pytest.raises(LookupValueError):
            TestModel.objects.filter(created_at__month=(2024, 0))

    def test_day_validates_date(self):
        """Test that day lookup validates the date."""
        registered_tables()

        # Feb 30 doesn't exist
        with pytest.raises(LookupValueError):
            TestModel.objects.filter(created_at__day=(2024, 2, 30))

    def test_date_part_lookups_on_date_field(self):
        """Test date part lookups on date field (not datetime)."""
        registered_tables()
        ir = TestModel.objects.filter(birth_date__year=1990).to_ir()
        conditions = get_and_conditions(ir)

        assert len(conditions) == 2


class TestInvalidLookups:
    """Test error handling for invalid lookups."""

    def test_unknown_lookup_raises(self):
        """Test that unknown lookup raises LookupError."""
        registered_tables()

        with pytest.raises(LookupError):
            TestModel.objects.filter(name__unknown="test")

    def test_string_lookup_on_numeric_field_raises(self):
        """Test that string lookup on numeric field raises error."""
        registered_tables()

        with pytest.raises(LookupError):
            TestModel.objects.filter(age__contains="18")

    def test_date_part_lookup_on_string_field_raises(self):
        """Test that date part lookup on string field raises error."""
        registered_tables()

        with pytest.raises(LookupError):
            TestModel.objects.filter(name__year=2024)

    def test_nonexistent_field_raises(self):
        """Test that nonexistent field raises FieldError."""
        registered_tables()

        with pytest.raises(FieldError):
            TestModel.objects.filter(nonexistent="value")


class TestMultipleFilters:
    """Test combining multiple filters."""

    def test_multiple_filters_same_field(self):
        """Test multiple filters on the same field."""
        registered_tables()
        ir = TestModel.objects.filter(age__gte=18, age__lte=65).to_ir()
        conditions = get_and_conditions(ir)

        assert len(conditions) == 2

    def test_multiple_filters_different_fields(self):
        """Test multiple filters on different fields."""
        registered_tables()
        ir = TestModel.objects.filter(name__icontains="test", age__gte=18).to_ir()
        conditions = get_and_conditions(ir)

        assert len(conditions) == 2

    def test_chained_filter_calls(self):
        """Test chaining filter() calls."""
        registered_tables()
        # Manager.filter() returns SelectQuery, chain with filter()
        query = TestModel.objects.filter(name__icontains="test").filter(age__gte=18)
        ir = query.to_ir()
        conditions = get_and_conditions(ir)

        assert len(conditions) == 2


class TestResolveColumnMeta:
    """Test _resolve_column_meta function."""

    def test_resolves_existing_field(self):
        """Test resolving metadata for existing field."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "name")

        assert meta.name == "name"
        assert meta.python_type == str

    def test_resolves_pk_field(self):
        """Test resolving metadata for primary key field."""
        registered_tables()
        meta = _resolve_column_meta(TestModel, "id")

        assert meta.primary_key is True

    def test_raises_for_nonexistent_field(self):
        """Test that nonexistent field raises FieldError."""
        registered_tables()

        with pytest.raises(FieldError):
            _resolve_column_meta(TestModel, "nonexistent")

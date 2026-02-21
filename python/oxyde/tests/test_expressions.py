"""Tests for F() and Q() expression composition."""

from __future__ import annotations

import pytest

from oxyde import Field, Model
from oxyde.models.registry import clear_registry, registered_tables
from oxyde.queries import F
from oxyde.queries.expressions import (
    _coerce_expression,
    _Expression,
    _serialize_value_for_ir,
)
from oxyde.queries.q import Q


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


class TestModel(Model):
    """Test model for expression tests."""

    id: int | None = Field(default=None, db_pk=True)
    name: str
    age: int = 0
    balance: int = 0
    score: float = 0.0
    is_active: bool = True

    class Meta:
        is_table = True


class TestFExpressionBasics:
    """Test basic F() expression functionality."""

    def test_f_creates_column_expression(self):
        """Test that F() creates a column expression."""
        f = F("balance")
        assert f.name == "balance"
        assert f._expression._expr["type"] == "column"
        assert f._expression._expr["name"] == "balance"

    def test_f_serializes_for_ir(self):
        """Test F() serializes correctly for IR."""
        from oxyde.queries.expressions import _serialize_value_for_ir

        # F("balance") + 100 should serialize to expression dict
        expr_value = F("balance") + 100
        serialized = _serialize_value_for_ir(expr_value)

        assert "__expr__" in serialized
        expr = serialized["__expr__"]
        assert expr["type"] == "op"
        assert expr["op"] == "add"
        assert expr["lhs"]["type"] == "column"
        assert expr["lhs"]["name"] == "balance"
        assert expr["rhs"]["type"] == "value"
        assert expr["rhs"]["value"] == 100


class TestFArithmeticOperations:
    """Test F() arithmetic operations."""

    def test_f_add(self):
        """Test F() + value."""
        expr = F("value") + 10
        assert isinstance(expr, _Expression)
        assert expr._expr["op"] == "add"

    def test_f_radd(self):
        """Test value + F()."""
        expr = 10 + F("value")
        assert isinstance(expr, _Expression)
        assert expr._expr["op"] == "add"

    def test_f_sub(self):
        """Test F() - value."""
        expr = F("value") - 5
        assert isinstance(expr, _Expression)
        assert expr._expr["op"] == "sub"

    def test_f_rsub(self):
        """Test value - F()."""
        expr = 100 - F("value")
        assert isinstance(expr, _Expression)
        assert expr._expr["op"] == "sub"
        # LHS should be value, RHS should be column
        assert expr._expr["lhs"]["type"] == "value"
        assert expr._expr["rhs"]["type"] == "column"

    def test_f_mul(self):
        """Test F() * value."""
        expr = F("value") * 2
        assert isinstance(expr, _Expression)
        assert expr._expr["op"] == "mul"

    def test_f_rmul(self):
        """Test value * F()."""
        expr = 2 * F("value")
        assert isinstance(expr, _Expression)
        assert expr._expr["op"] == "mul"

    def test_f_div(self):
        """Test F() / value."""
        expr = F("value") / 2
        assert isinstance(expr, _Expression)
        assert expr._expr["op"] == "div"

    def test_f_rdiv(self):
        """Test value / F()."""
        expr = 100 / F("value")
        assert isinstance(expr, _Expression)
        assert expr._expr["op"] == "div"

    def test_f_neg(self):
        """Test -F()."""
        expr = -F("value")
        assert isinstance(expr, _Expression)
        assert expr._expr["type"] == "neg"


class TestFNestedExpressions:
    """Test nested F() expressions."""

    def test_f_plus_f(self):
        """Test F() + F()."""
        expr = F("a") + F("b")
        assert expr._expr["op"] == "add"
        assert expr._expr["lhs"]["type"] == "column"
        assert expr._expr["rhs"]["type"] == "column"

    def test_complex_expression(self):
        """Test complex expression: (F("a") + F("b")) * 2."""
        expr = (F("a") + F("b")) * 2
        assert expr._expr["op"] == "mul"
        assert expr._expr["lhs"]["op"] == "add"
        assert expr._expr["rhs"]["type"] == "value"

    def test_very_nested_expression(self):
        """Test deeply nested expression."""
        expr = ((F("a") + 1) * 2 - F("b")) / 10
        assert expr._expr["op"] == "div"
        assert expr._expr["lhs"]["op"] == "sub"

    def test_combined_f_operations(self):
        """Test combined F() operations serialize correctly."""
        from oxyde.queries.expressions import _serialize_value_for_ir

        expr_value = F("score") * 1.1 + 10
        serialized = _serialize_value_for_ir(expr_value)

        expr = serialized["__expr__"]
        assert expr["op"] == "add"


class TestCoerceExpression:
    """Test _coerce_expression function."""

    def test_coerce_expression_from_expression(self):
        """Test coercing an Expression returns itself."""
        orig = _Expression({"type": "test"})
        coerced = _coerce_expression(orig)
        assert coerced is orig

    def test_coerce_expression_from_f(self):
        """Test coercing F() extracts its expression."""
        f = F("name")
        coerced = _coerce_expression(f)
        assert isinstance(coerced, _Expression)
        assert coerced._expr["type"] == "column"

    def test_coerce_expression_from_value(self):
        """Test coercing a value creates value expression."""
        coerced = _coerce_expression(42)
        assert isinstance(coerced, _Expression)
        assert coerced._expr["type"] == "value"
        assert coerced._expr["value"] == 42


class TestSerializeValueForIR:
    """Test _serialize_value_for_ir function."""

    def test_serialize_expression(self):
        """Test serializing an Expression."""
        expr = F("value") + 1
        serialized = _serialize_value_for_ir(expr)
        assert "__expr__" in serialized

    def test_serialize_f(self):
        """Test serializing F() directly."""
        f = F("value")
        serialized = _serialize_value_for_ir(f)
        assert "__expr__" in serialized

    def test_serialize_plain_value(self):
        """Test serializing plain value."""
        assert _serialize_value_for_ir(42) == 42
        assert _serialize_value_for_ir("test") == "test"

    def test_serialize_list(self):
        """Test serializing list with expressions."""
        lst = [1, F("value"), 3]
        serialized = _serialize_value_for_ir(lst)
        assert serialized[0] == 1
        assert "__expr__" in serialized[1]
        assert serialized[2] == 3

    def test_serialize_dict(self):
        """Test serializing dict with expressions."""
        d = {"a": 1, "b": F("value")}
        serialized = _serialize_value_for_ir(d)
        assert serialized["a"] == 1
        assert "__expr__" in serialized["b"]

    def test_serialize_datetime_types(self):
        """Test serializing datetime types that msgpack cannot handle."""
        from datetime import date, datetime, time, timedelta

        dt = datetime(2024, 1, 15, 12, 30, 45)
        assert _serialize_value_for_ir(dt) == "2024-01-15T12:30:45"

        d = date(2024, 1, 15)
        assert _serialize_value_for_ir(d) == "2024-01-15"

        t = time(12, 30, 45)
        assert _serialize_value_for_ir(t) == "12:30:45"

        td = timedelta(hours=1, minutes=30)
        assert _serialize_value_for_ir(td) == 5400.0  # total_seconds

    def test_serialize_uuid_decimal(self):
        """Test serializing UUID and Decimal types."""
        from decimal import Decimal
        from uuid import UUID

        u = UUID("12345678-1234-5678-1234-567812345678")
        assert _serialize_value_for_ir(u) == "12345678-1234-5678-1234-567812345678"

        dec = Decimal("123.45")
        assert _serialize_value_for_ir(dec) == "123.45"


class TestQExpressionBasics:
    """Test basic Q() expression functionality."""

    def test_q_with_kwargs(self):
        """Test Q() with keyword arguments."""
        registered_tables()
        q = Q(name="test")
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["field"] == "name"
        assert node["value"] == "test"

    def test_q_empty(self):
        """Test empty Q()."""
        q = Q()
        node = q.to_filter_node(TestModel)
        assert node is None

    def test_q_with_lookup(self):
        """Test Q() with lookup."""
        registered_tables()
        q = Q(age__gte=18)
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["operator"] == ">="

    def test_q_repr(self):
        """Test Q() string representation."""
        q = Q(name="test")
        assert "name" in repr(q)


class TestQAndComposition:
    """Test Q() AND composition."""

    def test_q_and_q(self):
        """Test Q() & Q()."""
        registered_tables()
        q = Q(name="test") & Q(age__gte=18)
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["type"] == "and"
        assert len(node["conditions"]) == 2

    def test_multiple_and(self):
        """Test multiple AND operations."""
        registered_tables()
        q = Q(name="test") & Q(age__gte=18) & Q(is_active=True)
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["type"] == "and"

    def test_q_and_with_empty(self):
        """Test Q() & Q() where one is empty."""
        registered_tables()
        q = Q(name="test") & Q()
        node = q.to_filter_node(TestModel)

        # Should simplify to just the non-empty condition
        assert node is not None


class TestQOrComposition:
    """Test Q() OR composition."""

    def test_q_or_q(self):
        """Test Q() | Q()."""
        registered_tables()
        q = Q(name="test") | Q(name="other")
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["type"] == "or"
        assert len(node["conditions"]) == 2

    def test_multiple_or(self):
        """Test multiple OR operations."""
        registered_tables()
        q = Q(name="a") | Q(name="b") | Q(name="c")
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["type"] == "or"


class TestQNotComposition:
    """Test Q() NOT composition."""

    def test_q_not(self):
        """Test ~Q()."""
        registered_tables()
        q = ~Q(is_active=True)
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["type"] == "not"

    def test_double_not(self):
        """Test ~~Q() (double negation)."""
        registered_tables()
        q = ~~Q(is_active=True)
        node = q.to_filter_node(TestModel)

        assert node is not None
        # Double NOT should result in NOT(NOT(...))
        assert node["type"] == "not"


class TestQComplexComposition:
    """Test complex Q() compositions."""

    def test_and_or_mixed(self):
        """Test (Q() & Q()) | Q()."""
        registered_tables()
        q = (Q(name="test") & Q(age__gte=18)) | Q(is_active=False)
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["type"] == "or"

    def test_or_and_mixed(self):
        """Test (Q() | Q()) & Q()."""
        registered_tables()
        q = (Q(name="a") | Q(name="b")) & Q(is_active=True)
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["type"] == "and"

    def test_not_and(self):
        """Test ~(Q() & Q())."""
        registered_tables()
        q = ~(Q(name="test") & Q(age__gte=18))
        node = q.to_filter_node(TestModel)

        assert node is not None
        assert node["type"] == "not"

    def test_deeply_nested(self):
        """Test deeply nested Q expressions."""
        registered_tables()
        q = (
            (Q(name="a") | Q(name="b"))
            & (Q(age__gte=18) | Q(is_active=True))
            & ~Q(balance=0)
        )
        node = q.to_filter_node(TestModel)

        assert node is not None


class TestQInQuery:
    """Test Q() usage in queries."""

    def test_filter_with_q_single(self):
        """Test filter with single Q expression."""
        registered_tables()
        ir = TestModel.objects.filter(Q(name="test")).to_ir()

        assert "filter_tree" in ir

    def test_filter_with_q_multiple(self):
        """Test filter with multiple Q expressions."""
        registered_tables()
        ir = TestModel.objects.filter(Q(name="test"), Q(age__gte=18)).to_ir()

        assert ir is not None

    def test_filter_with_q_and_kwargs(self):
        """Test filter with Q and kwargs."""
        registered_tables()
        ir = TestModel.objects.filter(Q(name="test"), is_active=True).to_ir()

        assert ir is not None

    def test_exclude_negates(self):
        """Test exclude() creates negated conditions."""
        registered_tables()
        query = TestModel.objects.exclude(is_active=True)
        ir = query.to_ir()

        assert ir is not None


class TestQValidation:
    """Test Q() validation."""

    def test_q_with_invalid_field_raises(self):
        """Test Q() with invalid field raises error."""
        registered_tables()
        from oxyde.exceptions import FieldError

        q = Q(nonexistent="value")
        with pytest.raises(FieldError):
            q.to_filter_node(TestModel)

    def test_q_with_invalid_lookup_raises(self):
        """Test Q() with invalid lookup raises error."""
        registered_tables()
        from oxyde.exceptions import FieldLookupError

        q = Q(name__invalid="value")
        with pytest.raises(FieldLookupError):
            q.to_filter_node(TestModel)

    def test_q_and_with_non_q_raises(self):
        """Test Q() & non-Q raises TypeError."""
        with pytest.raises(TypeError):
            Q(name="test") & "not a Q"

    def test_q_or_with_non_q_raises(self):
        """Test Q() | non-Q raises TypeError."""
        with pytest.raises(TypeError):
            Q(name="test") | "not a Q"

    def test_q_positional_and_kwargs_raises(self):
        """Test Q() with both positional and kwargs raises."""
        with pytest.raises(ValueError):
            Q({"field": "name"}, name="test")


class TestQMultipleKwargs:
    """Test Q() with multiple kwargs."""

    def test_q_multiple_kwargs_creates_and(self):
        """Test Q(a=1, b=2) creates AND of conditions."""
        registered_tables()
        q = Q(name="test", age=25)
        node = q.to_filter_node(TestModel)

        assert node is not None
        # Multiple kwargs should be ANDed together
        assert node["type"] == "and"

"""F-expressions for database-side field references and arithmetic.

F() allows referencing database columns in UPDATE statements for atomic
operations that execute entirely in the database, avoiding race conditions.

Basic Usage:
    F("price")           → Reference column "price"
    F("price") * 1.1     → price * 1.1
    F("count") + 1       → count + 1
    F("a") + F("b")      → a + b

Supported Operations:
    F("x") + y   → Addition
    F("x") - y   → Subtraction
    F("x") * y   → Multiplication
    F("x") / y   → Division
    -F("x")      → Negation

    Operands can be: F(), numbers, or other _Expression objects.

Use Cases:
    # Atomic increment (no race condition)
    await User.objects.filter(id=1).update(login_count=F("login_count") + 1)

    # Percentage increase
    await Product.objects.filter(category="sale").update(price=F("price") * 0.9)

    # Combine columns
    await Order.objects.update(total=F("price") * F("quantity"))

Internal Structure:
    F("name") creates:
        _Expression({"type": "column", "name": "name"})

    F("x") + 5 creates:
        _Expression({
            "type": "op",
            "op": "add",
            "lhs": {"type": "column", "name": "x"},
            "rhs": {"type": "value", "value": 5}
        })

    _serialize_value_for_ir() wraps expressions in {"__expr__": ...}
    for the Rust IR parser to recognize them.
"""

from __future__ import annotations

from datetime import date, datetime, time, timedelta
from decimal import Decimal
from typing import Any
from uuid import UUID

# Types that msgpack cannot serialize - convert to str
_STRINGIFY_TYPES = (datetime, date, time, UUID, Decimal)


class _Expression:
    """Represents an arithmetic expression usable in mutation statements."""

    __slots__ = ("_expr",)

    def __init__(self, expr: dict[str, Any]):
        self._expr = expr

    def _binary(self, op: str, other: Any) -> _Expression:
        rhs = _coerce_expression(other)
        return _Expression(
            {
                "type": "op",
                "op": op,
                "lhs": self._expr,
                "rhs": rhs._expr,
            }
        )

    def __add__(self, other: Any) -> _Expression:
        return self._binary("add", other)

    def __radd__(self, other: Any) -> _Expression:
        return _coerce_expression(other)._binary("add", self)

    def __sub__(self, other: Any) -> _Expression:
        return self._binary("sub", other)

    def __rsub__(self, other: Any) -> _Expression:
        return _coerce_expression(other)._binary("sub", self)

    def __mul__(self, other: Any) -> _Expression:
        return self._binary("mul", other)

    def __rmul__(self, other: Any) -> _Expression:
        return _coerce_expression(other)._binary("mul", self)

    def __truediv__(self, other: Any) -> _Expression:
        return self._binary("div", other)

    def __rtruediv__(self, other: Any) -> _Expression:
        return _coerce_expression(other)._binary("div", self)

    def __neg__(self) -> _Expression:
        return _Expression({"type": "neg", "expr": self._expr})


def _coerce_expression(value: Any) -> _Expression:
    """Coerce a value to an _Expression."""
    if isinstance(value, _Expression):
        return value
    if isinstance(value, F):
        return value._expression
    return _Expression({"type": "value", "value": value})


class F:
    """Field expression helper for arithmetic updates."""

    __slots__ = ("name", "_expression")

    def __init__(self, name: str):
        self.name = name
        self._expression = _Expression({"type": "column", "name": name})

    def __add__(self, other: Any) -> _Expression:
        return self._expression.__add__(other)

    def __radd__(self, other: Any) -> _Expression:
        return self._expression.__radd__(other)

    def __sub__(self, other: Any) -> _Expression:
        return self._expression.__sub__(other)

    def __rsub__(self, other: Any) -> _Expression:
        return self._expression.__rsub__(other)

    def __mul__(self, other: Any) -> _Expression:
        return self._expression.__mul__(other)

    def __rmul__(self, other: Any) -> _Expression:
        return self._expression.__rmul__(other)

    def __truediv__(self, other: Any) -> _Expression:
        return self._expression.__truediv__(other)

    def __rtruediv__(self, other: Any) -> _Expression:
        return self._expression.__rtruediv__(other)

    def __neg__(self) -> _Expression:
        return -self._expression


def _serialize_value_for_ir(value: Any) -> Any:
    """Serialize values for IR, handling expressions and non-msgpack types."""
    if isinstance(value, _Expression):
        return {"__expr__": value._expr}
    if isinstance(value, F):
        return {"__expr__": value._expression._expr}
    if isinstance(value, list):
        return [_serialize_value_for_ir(item) for item in value]
    if isinstance(value, dict):
        return {key: _serialize_value_for_ir(val) for key, val in value.items()}
    # Types that msgpack cannot serialize
    if isinstance(value, _STRINGIFY_TYPES):
        return str(value)
    if isinstance(value, timedelta):
        return value.total_seconds()
    return value


__all__ = ["F", "_Expression", "_coerce_expression", "_serialize_value_for_ir"]

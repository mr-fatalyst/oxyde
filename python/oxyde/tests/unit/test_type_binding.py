"""Test that query parameters are bound with correct sea_query types for all dialects.

Uses .sql(with_types=True) to inspect (type_tag, value) tuples without a database.
This validates the full Python → serialize → msgpack → Rust → sea_query::Value pipeline.
"""

from __future__ import annotations

from datetime import date, datetime, time, timedelta
from decimal import Decimal
from uuid import UUID

import pytest

from oxyde import Field, Model

DIALECTS = ["postgres", "sqlite", "mysql"]

# -- Test fixtures (values used across tests) --

DT = datetime(2024, 1, 15, 12, 30, 0)
D = date(2024, 1, 15)
T = time(12, 30, 0)
TD = timedelta(seconds=90)
U = UUID("550e8400-e29b-41d4-a716-446655440000")
DEC = Decimal("99.99")


# -- Model covering all supported types --


class TypeModel(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")
    age: int = Field(default=0)
    score: float = Field(default=0.0)
    active: bool = Field(default=True)
    created: datetime | None = Field(default=None, db_nullable=True)
    birthday: date | None = Field(default=None, db_nullable=True)
    wake_up: time | None = Field(default=None, db_nullable=True)
    duration: timedelta | None = Field(default=None, db_nullable=True)
    uuid_val: UUID | None = Field(default=None, db_nullable=True)
    price: Decimal | None = Field(default=None, db_nullable=True)
    data: dict | None = Field(default=None, db_nullable=True)
    blob: bytes | None = Field(default=None, db_nullable=True)
    tags: list[str] | None = Field(default=None, db_nullable=True)

    class Meta:
        is_table = True
        table_name = "type_model"


# ---- Filter binding: type correctness ----

FILTER_CASES = [
    ("int", {"age": 25}, [("BigInt", 25)]),
    ("str", {"name": "Alice"}, [("String", "Alice")]),
    ("bool_true", {"active": True}, [("Bool", True)]),
    ("bool_false", {"active": False}, [("Bool", False)]),
    ("float", {"score": 3.14}, [("Double", 3.14)]),
    ("datetime", {"created": DT}, [("ChronoDateTime", "2024-01-15 12:30:00")]),
    ("date", {"birthday": D}, [("ChronoDate", "2024-01-15")]),
    ("time", {"wake_up": T}, [("ChronoTime", "12:30:00")]),
    (
        "timedelta",
        {"duration": TD},
        [("BigInt", int(TD.total_seconds() * 1_000_000))],
    ),
    ("uuid", {"uuid_val": U}, [("Uuid", str(U))]),
    ("decimal", {"price": DEC}, [("Decimal", str(DEC))]),
    ("bytes", {"blob": b"hello"}, [("Bytes", b"hello")]),
    ("dict_json", {"data": {"key": "val"}}, [("Json", '{"key":"val"}')]),
    (
        "list_str_array",
        {"tags": ["a", "b"]},
        [("Array", ("String", ["a", "b"]))],
    ),
]


@pytest.mark.parametrize("dialect", DIALECTS)
@pytest.mark.parametrize(
    "name,filters,expected",
    FILTER_CASES,
    ids=[c[0] for c in FILTER_CASES],
)
def test_filter_binding(dialect, name, filters, expected):
    _, params = TypeModel.objects.filter(**filters).sql(
        dialect=dialect, with_types=True
    )
    assert params == expected, f"[{dialect}] {name}: {params} != {expected}"


# ---- P1 regression: str field must not reinterpret ISO datetime ----

STR_REGRESSION_CASES = [
    ("rfc3339_utc", "2024-01-15T12:30:00Z"),
    ("rfc3339_offset", "2024-01-15T12:30:00+03:00"),
    ("iso_naive", "2024-01-15T12:30:00"),
    ("date_only_str", "2024-01-15"),
]


@pytest.mark.parametrize("dialect", DIALECTS)
@pytest.mark.parametrize(
    "name,value",
    STR_REGRESSION_CASES,
    ids=[c[0] for c in STR_REGRESSION_CASES],
)
def test_str_field_no_datetime_reinterpretation(dialect, name, value):
    """ISO datetime strings in str fields must bind as String, not datetime."""
    _, params = TypeModel.objects.filter(name=value).sql(
        dialect=dialect, with_types=True
    )
    assert params[0][0] == "String", (
        f"[{dialect}] str field with '{value}' bound as {params[0][0]}, expected String"
    )


# ---- Filter operators ----

OPERATOR_CASES = [
    ("gte", {"age__gte": 18}, [("BigInt", 18)]),
    ("lte", {"age__lte": 65}, [("BigInt", 65)]),
    ("gt", {"score__gt": 3.0}, [("Double", 3.0)]),
    ("lt", {"score__lt": 5.0}, [("Double", 5.0)]),
    ("contains", {"name__contains": "ali"}, [("String", "%ali%")]),
    ("startswith", {"name__startswith": "Al"}, [("String", "Al%")]),
    ("endswith", {"name__endswith": "ce"}, [("String", "%ce")]),
    ("in_int", {"age__in": [20, 30, 40]}, [("BigInt", 20), ("BigInt", 30), ("BigInt", 40)]),
    ("in_str", {"name__in": ["Alice", "Bob"]}, [("String", "Alice"), ("String", "Bob")]),
]


@pytest.mark.parametrize("dialect", DIALECTS)
@pytest.mark.parametrize(
    "name,filters,expected",
    OPERATOR_CASES,
    ids=[c[0] for c in OPERATOR_CASES],
)
def test_operator_binding(dialect, name, filters, expected):
    _, params = TypeModel.objects.filter(**filters).sql(
        dialect=dialect, with_types=True
    )
    assert params == expected, f"[{dialect}] {name}: {params} != {expected}"


# ---- Multiple filters ----


@pytest.mark.parametrize("dialect", DIALECTS)
def test_multiple_filters_order(dialect):
    _, params = TypeModel.objects.filter(age=25, name="Alice", active=True).sql(
        dialect=dialect, with_types=True
    )
    assert len(params) == 3
    assert params[0] == ("BigInt", 25)
    assert params[1] == ("String", "Alice")
    assert params[2] == ("Bool", True)


# ---- NULL binding ----


@pytest.mark.parametrize("dialect", DIALECTS)
def test_null_filter(dialect):
    _, params = TypeModel.objects.filter(name__isnull=True).sql(
        dialect=dialect, with_types=True
    )
    # isnull=True generates IS NULL in SQL, no params
    assert params == []


# ---- SQL syntax per dialect ----


@pytest.mark.parametrize(
    "dialect,placeholder,quote",
    [
        ("postgres", "$1", '"'),
        ("sqlite", "?", '"'),
        ("mysql", "?", "`"),
    ],
)
def test_sql_syntax(dialect, placeholder, quote):
    sql, _ = TypeModel.objects.filter(age=25).sql(dialect=dialect)
    assert placeholder in sql, f"[{dialect}] expected {placeholder} in: {sql}"
    assert f"{quote}age{quote}" in sql, f"[{dialect}] expected {quote}age{quote} in: {sql}"


# ---- Backward compatibility: with_types=False (default) ----


def test_without_types_returns_plain_values():
    _, params = TypeModel.objects.filter(age=25, name="Alice").sql()
    assert params == [25, "Alice"]
    assert isinstance(params[0], int)
    assert isinstance(params[1], str)

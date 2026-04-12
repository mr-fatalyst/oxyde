"""Round-trip tests for all supported field types.

Tests create -> get -> assert for each Python type through the full pipeline:
Python -> msgpack IR -> Rust -> SQL -> DB -> Rust convert -> msgpack -> Python -> Pydantic
"""
from __future__ import annotations

import uuid

import pytest
import pytest_asyncio

from datetime import date, datetime, time, timedelta
from decimal import Decimal
from uuid import UUID

from oxyde.queries.raw import execute_raw

from .conftest import AllTypes, BytesModel, NullableTypes, TdModel


# ── Models for this file are defined in conftest.py ────────────────────
# AllTypes, NullableTypes, BytesModel, TdModel
# Tables created by create_tables() via conftest db fixture.


# ── Basic type round-trips ──────────────────────────────────────────────


class TestIntRoundTrip:
    @pytest.mark.asyncio
    async def test_positive(self, db):
        obj = await AllTypes.objects.create(int_val=42, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.int_val == 42
        assert type(fetched.int_val) is int

    @pytest.mark.asyncio
    async def test_zero(self, db):
        obj = await AllTypes.objects.create(int_val=0, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.int_val == 0

    @pytest.mark.asyncio
    async def test_negative(self, db):
        obj = await AllTypes.objects.create(int_val=-100, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.int_val == -100

    @pytest.mark.asyncio
    async def test_large(self, db):
        big = 2**53 - 1  # max safe integer for JSON
        obj = await AllTypes.objects.create(int_val=big, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.int_val == big


class TestStrRoundTrip:
    @pytest.mark.asyncio
    async def test_basic(self, db):
        obj = await AllTypes.objects.create(str_val="hello world", using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.str_val == "hello world"
        assert type(fetched.str_val) is str

    @pytest.mark.asyncio
    async def test_empty(self, db):
        obj = await AllTypes.objects.create(str_val="", using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.str_val == ""

    @pytest.mark.asyncio
    async def test_unicode(self, db):
        text = "Привет мир 🌍 日本語"
        obj = await AllTypes.objects.create(str_val=text, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.str_val == text

    @pytest.mark.asyncio
    async def test_special_chars(self, db):
        text = "line1\nline2\ttab\\backslash'quote\"double"
        obj = await AllTypes.objects.create(str_val=text, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.str_val == text


class TestFloatRoundTrip:
    @pytest.mark.asyncio
    async def test_basic(self, db):
        obj = await AllTypes.objects.create(float_val=3.14, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.float_val == pytest.approx(3.14)
        assert type(fetched.float_val) is float

    @pytest.mark.asyncio
    async def test_zero(self, db):
        obj = await AllTypes.objects.create(float_val=0.0, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.float_val == 0.0

    @pytest.mark.asyncio
    async def test_negative(self, db):
        obj = await AllTypes.objects.create(float_val=-2.718, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.float_val == pytest.approx(-2.718)

    @pytest.mark.asyncio
    async def test_very_small(self, db):
        obj = await AllTypes.objects.create(float_val=1e-10, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.float_val == pytest.approx(1e-10)

    @pytest.mark.asyncio
    async def test_very_large(self, db):
        obj = await AllTypes.objects.create(float_val=1e15, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.float_val == pytest.approx(1e15)


class TestBoolRoundTrip:
    @pytest.mark.asyncio
    async def test_true(self, db):
        obj = await AllTypes.objects.create(bool_val=True, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.bool_val is True

    @pytest.mark.asyncio
    async def test_false(self, db):
        obj = await AllTypes.objects.create(bool_val=False, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.bool_val is False


# ── Rich type round-trips ──────────────────────────────────────────────


class TestDatetimeRoundTrip:
    @pytest.mark.asyncio
    async def test_basic(self, db):
        dt = datetime(2024, 1, 15, 12, 30, 45)
        obj = await AllTypes.objects.create(datetime_val=dt, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.datetime_val == dt
        assert type(fetched.datetime_val) is datetime

    @pytest.mark.asyncio
    async def test_midnight(self, db):
        dt = datetime(2024, 6, 1, 0, 0, 0)
        obj = await AllTypes.objects.create(datetime_val=dt, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.datetime_val == dt

    @pytest.mark.asyncio
    async def test_with_microseconds(self, db):
        dt = datetime(2024, 1, 15, 12, 30, 45, 123456)
        obj = await AllTypes.objects.create(datetime_val=dt, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.datetime_val == dt


class TestDateRoundTrip:
    @pytest.mark.asyncio
    async def test_basic(self, db):
        d = date(2024, 1, 15)
        obj = await AllTypes.objects.create(date_val=d, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.date_val == d
        assert type(fetched.date_val) is date

    @pytest.mark.asyncio
    async def test_leap_day(self, db):
        d = date(2024, 2, 29)
        obj = await AllTypes.objects.create(date_val=d, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.date_val == d


class TestTimeRoundTrip:
    @pytest.mark.asyncio
    async def test_basic(self, db):
        t = time(12, 30, 45)
        obj = await AllTypes.objects.create(time_val=t, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.time_val == t
        assert type(fetched.time_val) is time

    @pytest.mark.asyncio
    async def test_midnight(self, db):
        t = time(0, 0, 0)
        obj = await AllTypes.objects.create(time_val=t, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.time_val == t

    @pytest.mark.asyncio
    async def test_with_microseconds(self, db):
        t = time(12, 30, 45, 123456)
        obj = await AllTypes.objects.create(time_val=t, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.time_val == t


class TestUuidRoundTrip:
    @pytest.mark.asyncio
    async def test_basic(self, db):
        u = UUID("12345678-1234-5678-1234-567812345678")
        obj = await AllTypes.objects.create(uuid_val=u, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.uuid_val == u
        assert type(fetched.uuid_val) is UUID

    @pytest.mark.asyncio
    async def test_random(self, db):
        u = uuid.uuid4()
        obj = await AllTypes.objects.create(uuid_val=u, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.uuid_val == u


class TestDecimalRoundTrip:
    @pytest.mark.asyncio
    async def test_basic(self, db):
        d = Decimal("123.45")
        obj = await AllTypes.objects.create(decimal_val=d, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.decimal_val == d
        assert type(fetched.decimal_val) is Decimal

    @pytest.mark.asyncio
    async def test_zero(self, db):
        d = Decimal("0")
        obj = await AllTypes.objects.create(decimal_val=d, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.decimal_val == d

    @pytest.mark.asyncio
    async def test_negative(self, db):
        d = Decimal("-99.99")
        obj = await AllTypes.objects.create(decimal_val=d, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.decimal_val == d

    @pytest.mark.asyncio
    async def test_high_precision(self, db):
        d = Decimal("123456789.123456789")
        obj = await AllTypes.objects.create(decimal_val=d, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.decimal_val == d


class TestJsonRoundTrip:
    @pytest.mark.asyncio
    async def test_basic(self, db):
        data = {"key": "value"}
        obj = await AllTypes.objects.create(json_val=data, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.json_val == data
        assert type(fetched.json_val) is dict

    @pytest.mark.asyncio
    async def test_nested(self, db):
        data = {"users": [{"name": "Alice", "age": 30}], "count": 1}
        obj = await AllTypes.objects.create(json_val=data, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.json_val == data

    @pytest.mark.asyncio
    async def test_empty_dict(self, db):
        obj = await AllTypes.objects.create(json_val={}, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.json_val == {}


# ── NULL round-trips ────────────────────────────────────────────────────


class TestNullRoundTrip:
    """All nullable fields should survive a round-trip as None.

    Uses seeded NULL rows since create() with all-None kwargs is rejected.
    """

    @pytest_asyncio.fixture
    async def db_with_nulls(self, db):
        """Seed a row with all NULLs via raw SQL."""
        await execute_raw(
            "INSERT INTO nullable_types (int_val, str_val, float_val, bool_val, "
            "datetime_val, date_val, time_val, uuid_val, decimal_val, json_val) "
            "VALUES (NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
            using=db.name,
        )
        return db

    @pytest.mark.asyncio
    async def test_all_nulls(self, db_with_nulls):
        fetched = await NullableTypes.objects.first(using=db_with_nulls.name)
        assert fetched.int_val is None
        assert fetched.str_val is None
        assert fetched.float_val is None
        assert fetched.bool_val is None
        assert fetched.datetime_val is None
        assert fetched.date_val is None
        assert fetched.time_val is None
        assert fetched.uuid_val is None
        assert fetched.decimal_val is None
        assert fetched.json_val is None


# ── bytes & timedelta round-trips ──────────────────────────────────────


class TestBytesRoundTrip:
    """bytes round-trip through native msgpack binary."""

    @pytest.mark.asyncio
    async def test_basic(self, db):
        obj = await BytesModel.objects.create(data=b"\x00\x01\x02\xff", using=db.name)
        fetched = await BytesModel.objects.get(id=obj.id, using=db.name)
        assert fetched.data == b"\x00\x01\x02\xff"
        assert type(fetched.data) is bytes


class TestTimedeltaRoundTrip:
    """timedelta round-trip: stored as BIGINT microseconds."""

    @pytest.mark.asyncio
    async def test_basic(self, db):
        td = timedelta(hours=1, minutes=30)
        obj = await TdModel.objects.create(duration=td, using=db.name)
        fetched = await TdModel.objects.get(id=obj.id, using=db.name)
        assert fetched.duration == td


class TestArrayRoundTrip:
    @pytest.mark.asyncio
    async def test_str_list(self, db):
        obj = await AllTypes.objects.create(
            str_list=["alpha", "beta", "gamma"], using=db.name
        )
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.str_list == ["alpha", "beta", "gamma"]
        assert type(fetched.str_list) is list

    @pytest.mark.asyncio
    async def test_int_list(self, db):
        obj = await AllTypes.objects.create(int_list=[1, 2, 3], using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.int_list == [1, 2, 3]
        assert type(fetched.int_list) is list

    @pytest.mark.asyncio
    async def test_empty_list(self, db):
        obj = await AllTypes.objects.create(str_list=[], using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.str_list == []

    @pytest.mark.asyncio
    async def test_decimal_list(self, db):
        vals = [Decimal("1.50"), Decimal("99.99")]
        obj = await AllTypes.objects.create(decimal_list=vals, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.decimal_list == vals

    @pytest.mark.asyncio
    async def test_null_list(self, db):
        obj = await AllTypes.objects.create(str_list=None, using=db.name)
        fetched = await AllTypes.objects.get(id=obj.id, using=db.name)
        assert fetched.str_list is None

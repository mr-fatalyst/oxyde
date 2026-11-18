"""Integration tests for explicit db_type fields across all dialects.

Tests create -> get -> assert for fields with explicit db_type,
covering dialect-specific SQL types (BYTEA vs BLOB, UUID, arrays, etc.).
One model per dialect, fixture selects by backend.
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone
from decimal import Decimal
from uuid import UUID

import pytest
import pytest_asyncio

from oxyde import AsyncDatabase, Field, Model, Q, disconnect_all
from oxyde.db.schema import create_tables
from oxyde.models.registry import clear_registry, register_table
from oxyde.queries.raw import execute_raw

from .conftest import _DIALECTS, _get_url


def _has(model: type, field: str) -> bool:
    """Check if a Pydantic model class has a field (class-level hasattr doesn't work)."""
    return field in model.model_fields


# ── Dialect-specific models ───────────────────────────────────────────


class PgDbTypes(Model):
    id: int | None = Field(default=None, db_pk=True)
    uuid_val: UUID | None = Field(default=None, db_nullable=True, db_type="UUID")
    jsonb_val: dict | None = Field(default=None, db_nullable=True, db_type="JSONB")
    varchar_val: str = Field(default="", db_type="VARCHAR(100)")
    numeric_val: Decimal | None = Field(
        default=None, db_nullable=True, db_type="NUMERIC(10,2)"
    )
    bytea_val: bytes | None = Field(default=None, db_nullable=True, db_type="BYTEA")
    ts_val: datetime | None = Field(
        default=None, db_nullable=True, db_type="TIMESTAMP"
    )
    tstz_val: datetime | None = Field(
        default=None, db_nullable=True, db_type="TIMESTAMPTZ"
    )
    varchar_arr: list[str] | None = Field(
        default=None, db_nullable=True, db_type="VARCHAR(100)[]"
    )
    uuid_arr: list[UUID] | None = Field(
        default=None, db_nullable=True, db_type="UUID[]"
    )
    int_arr: list[int] | None = Field(
        default=None, db_nullable=True, db_type="INTEGER[]"
    )
    smallint_val: int | None = Field(
        default=None, db_nullable=True, db_type="SMALLINT"
    )
    smallint_arr: list[int] | None = Field(
        default=None, db_nullable=True, db_type="SMALLINT[]"
    )
    char_arr: list[str] | None = Field(
        default=None, db_nullable=True, db_type="CHAR(3)[]"
    )
    real_arr: list[float] | None = Field(
        default=None, db_nullable=True, db_type="FLOAT(24)[]"
    )
    tstz_arr: list[datetime] | None = Field(
        default=None, db_nullable=True, db_type="TIMESTAMPTZ[]"
    )
    # Elements are JSON values: a str element is a JSON string, never parsed
    jsonb_arr: list[str | None] | None = Field(
        default=None, db_nullable=True, db_type="JSONB[]"
    )
    bytea_arr: list[bytes] | None = Field(default=None, db_nullable=True)

    class Meta:
        is_table = True
        table_name = "db_types_test"


class MysqlDbTypes(Model):
    id: int | None = Field(default=None, db_pk=True)
    json_val: dict | None = Field(default=None, db_nullable=True, db_type="JSON")
    varchar_val: str = Field(default="", db_type="VARCHAR(100)")
    decimal_val: Decimal | None = Field(
        default=None, db_nullable=True, db_type="DECIMAL(10,2)"
    )
    blob_val: bytes | None = Field(default=None, db_nullable=True, db_type="BLOB")
    ts_val: datetime | None = Field(
        default=None, db_nullable=True, db_type="DATETIME(6)"
    )
    char_val: str = Field(default="", db_type="CHAR(36)")

    class Meta:
        is_table = True
        table_name = "db_types_test"


class SqliteDbTypes(Model):
    id: int | None = Field(default=None, db_pk=True)
    text_val: str = Field(default="", db_type="TEXT")
    int_val: int = Field(default=0, db_type="INTEGER")
    real_val: float = Field(default=0.0, db_type="REAL")
    blob_val: bytes | None = Field(default=None, db_nullable=True, db_type="BLOB")

    class Meta:
        is_table = True
        table_name = "db_types_test"


DIALECT_MODELS = {
    "postgres": PgDbTypes,
    "mysql": MysqlDbTypes,
    "sqlite": SqliteDbTypes,
}


# ── Fixture ───────────────────────────────────────────────────────────


@pytest_asyncio.fixture(params=_DIALECTS)
async def typed_db(request, tmp_path, _pg_container, _mysql_container):
    url = _get_url(request.param, tmp_path, _pg_container, _mysql_container)
    model = DIALECT_MODELS[request.param]

    clear_registry()
    register_table(model, overwrite=True)

    database = AsyncDatabase(
        url, name=f"dbtype_{request.param}", overwrite=True
    )
    await database.connect()

    # Drop existing table before creating with new schema
    try:
        await execute_raw("DROP TABLE IF EXISTS db_types_test", using=database.name)
    except Exception:
        pass
    await create_tables(database)

    yield database, model
    await disconnect_all()


# ── Tests ─────────────────────────────────────────────────────────────


class TestDbTypes:
    @pytest.mark.asyncio
    async def test_varchar(self, typed_db):
        db, model = typed_db
        if not _has(model,"varchar_val"):
            pytest.skip("No varchar field on this dialect model")
        obj = await model.objects.create(varchar_val="hello world", using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert fetched.varchar_val == "hello world"

    @pytest.mark.asyncio
    async def test_numeric_decimal(self, typed_db):
        db, model = typed_db
        if not _has(model,"numeric_val") and not _has(model,"decimal_val"):
            pytest.skip("No numeric/decimal field on this dialect model")
        field = "numeric_val" if _has(model,"numeric_val") else "decimal_val"
        val = Decimal("123.45")
        obj = await model.objects.create(**{field: val}, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert getattr(fetched, field) == val

    @pytest.mark.asyncio
    async def test_bytes(self, typed_db):
        db, model = typed_db
        field = "bytea_val" if _has(model,"bytea_val") else "blob_val"
        data = b"\x00\x01\x02\xff"
        obj = await model.objects.create(**{field: data}, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert getattr(fetched, field) == data

    @pytest.mark.asyncio
    async def test_json(self, typed_db):
        db, model = typed_db
        field = None
        for name in ("jsonb_val", "json_val"):
            if _has(model,name):
                field = name
                break
        if field is None:
            pytest.skip("No JSON field on this dialect model")
        data = {"key": "value", "nested": [1, 2]}
        obj = await model.objects.create(**{field: data}, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert getattr(fetched, field) == data

    @pytest.mark.asyncio
    async def test_uuid(self, typed_db):
        db, model = typed_db
        if not _has(model,"uuid_val"):
            pytest.skip("No UUID field on this dialect model")
        val = UUID("12345678-1234-5678-1234-567812345678")
        obj = await model.objects.create(uuid_val=val, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert fetched.uuid_val == val

    @pytest.mark.asyncio
    async def test_timestamp(self, typed_db):
        db, model = typed_db
        field = None
        for name in ("ts_val",):
            if _has(model,name):
                field = name
                break
        if field is None:
            pytest.skip("No timestamp field on this dialect model")
        val = datetime(2024, 6, 15, 12, 30, 0)
        obj = await model.objects.create(**{field: val}, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert getattr(fetched, field) == val

    @pytest.mark.asyncio
    async def test_varchar_array(self, typed_db):
        db, model = typed_db
        if not _has(model,"varchar_arr"):
            pytest.skip("No varchar array on this dialect model")
        val = ["alpha", "beta"]
        obj = await model.objects.create(varchar_arr=val, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert fetched.varchar_arr == val

    @pytest.mark.asyncio
    async def test_uuid_array(self, typed_db):
        db, model = typed_db
        if not _has(model,"uuid_arr"):
            pytest.skip("No UUID array on this dialect model")
        val = [UUID("12345678-1234-5678-1234-567812345678")]
        obj = await model.objects.create(uuid_arr=val, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert fetched.uuid_arr == val

    @pytest.mark.asyncio
    async def test_int_array(self, typed_db):
        db, model = typed_db
        if not _has(model,"int_arr"):
            pytest.skip("No int array on this dialect model")
        val = [1, 2, 3]
        obj = await model.objects.create(int_arr=val, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert fetched.int_arr == val

    @pytest.mark.asyncio
    async def test_array_overrides_cast_filter_values(self, typed_db):
        db, model = typed_db
        if not _has(model, "char_arr"):
            pytest.skip("No array db_type overrides on this dialect model")
        await model.objects.create(
            int_arr=[1, 2], char_arr=["abc", "de"], real_arr=[1.5, 0.25], using=db.name
        )
        await model.objects.create(real_arr=[float("nan")], using=db.name)

        async def count(*args, **filters) -> int:
            return await model.objects.filter(*args, **filters).count(using=db.name)

        # integer[] has no = with the bigint[] values bind as, without a cast.
        assert await count(int_arr=[1, 2]) == 1
        assert await count(int_arr=[2, 1]) == 0
        assert await count(int_arr__in=[[1, 2], [3]]) == 1
        assert await count(char_arr=["abc", "de"]) == 1
        # FLOAT(24) is real: values are cast to real[], not double precision[].
        assert await count(real_arr=[1.5, 0.25]) == 1
        assert await count(real_arr=[float("nan")]) == 1
        # Values the column type cannot hold equal no stored value.
        assert await count(int_arr=[2**31]) == 0
        assert await count(int_arr__in=[[1, 2], [2**31]]) == 1
        assert await count(~Q(int_arr=[2**31])) == 1
        assert await count(real_arr=[1e300]) == 0

    @pytest.mark.asyncio
    async def test_bytea_array(self, typed_db):
        db, model = typed_db
        if not _has(model, "bytea_arr"):
            pytest.skip("No bytea array on this dialect model")
        val = [b"\x00ab", b"cd"]
        obj = await model.objects.create(bytea_arr=val, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert fetched.bytea_arr == val
        assert await model.objects.filter(bytea_arr=val).count(using=db.name) == 1

    @pytest.mark.asyncio
    async def test_timestamptz_array_elements(self, typed_db):
        db, model = typed_db
        if not _has(model, "tstz_arr"):
            pytest.skip("No timestamptz array on this dialect model")
        # Naive elements of a TIMESTAMPTZ[] column are taken as UTC (they
        # were stored as NULL).
        cest = timezone(timedelta(hours=2))
        obj = await model.objects.create(
            tstz_arr=[datetime(2024, 1, 1, 8, 0), datetime(2024, 1, 1, 10, 0, tzinfo=cest)],
            using=db.name,
        )
        fetched = await model.objects.get(id=obj.id, using=db.name)
        expected = datetime(2024, 1, 1, 8, 0, tzinfo=timezone.utc)
        assert fetched.tstz_arr == [expected, expected]

    @pytest.mark.asyncio
    async def test_jsonb_array_strings(self, typed_db):
        db, model = typed_db
        if not _has(model, "jsonb_arr"):
            pytest.skip("No jsonb array on this dialect model")
        # Strings are JSON strings, never parsed: text that is not JSON was
        # stored as NULL, and JSON text ("123", "null") changed type.
        strings = ['"hello"', "world", "123", "null", None]
        obj = await model.objects.create(jsonb_arr=strings, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert fetched.jsonb_arr == strings

        async def count(**filters) -> int:
            return await model.objects.filter(**filters).count(using=db.name)

        assert await count(jsonb_arr=strings) == 1
        assert await count(jsonb_arr=["hello", "world", "123", "null", None]) == 0

    @pytest.mark.asyncio
    @pytest.mark.parametrize("val", [-32768, -1, 0, 3, 32767, None])
    async def test_smallint(self, typed_db, val):
        db, model = typed_db
        if not _has(model, "smallint_val"):
            pytest.skip("No SMALLINT field on this dialect model")
        obj = await model.objects.create(smallint_val=val, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert fetched.smallint_val == val
        if val is not None:
            assert isinstance(fetched.smallint_val, int)

    @pytest.mark.asyncio
    @pytest.mark.parametrize("val", [[-32768, -1, 0, 3, 32767], [], None])
    async def test_smallint_array(self, typed_db, val):
        db, model = typed_db
        if not _has(model, "smallint_arr"):
            pytest.skip("No SMALLINT array on this dialect model")
        obj = await model.objects.create(smallint_arr=val, using=db.name)
        fetched = await model.objects.get(id=obj.id, using=db.name)
        assert fetched.smallint_arr == val
        if val is not None:
            assert all(isinstance(item, int) for item in fetched.smallint_arr)

    @pytest.mark.asyncio
    async def test_raw_smallint(self, typed_db):
        db, model = typed_db
        if not _has(model, "smallint_val"):
            pytest.skip("PostgreSQL-specific SMALLINT cast")
        rows = await execute_raw("SELECT 3::smallint AS v", using=db.name)
        assert rows == [{"v": 3}]
        assert isinstance(rows[0]["v"], int)

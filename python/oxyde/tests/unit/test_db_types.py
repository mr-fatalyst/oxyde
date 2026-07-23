"""Test that column_types specs are correctly computed for all field configurations.

Validates _compute_column_types() output — ColumnTypeSpec dicts sent to Rust
for typed binding and reading. One model per category, parametrized cases.
"""

from __future__ import annotations

from datetime import date, datetime, time, timedelta
from decimal import Decimal
from enum import Enum, IntEnum
from typing import Annotated
from uuid import UUID

import pytest

from oxyde import Field, Model
from oxyde.models.registry import clear_registry


class Status(Enum):
    DRAFT = "draft"
    PUBLISHED = "published"


class Priority(IntEnum):
    LOW = 1
    HIGH = 2


# ── Model with all type variations ────────────────────────────────────


class DbTypesModel(Model):
    id: int | None = Field(default=None, db_pk=True)

    # Inferred from python_type (no db_type)
    infer_str: str = Field(default="")
    infer_int: int = Field(default=0)
    infer_float: float = Field(default=0.0)
    infer_bool: bool = Field(default=True)
    infer_bytes: bytes | None = Field(default=None, db_nullable=True)
    infer_datetime: datetime | None = Field(default=None, db_nullable=True)
    infer_date: date | None = Field(default=None, db_nullable=True)
    infer_time: time | None = Field(default=None, db_nullable=True)
    infer_timedelta: timedelta | None = Field(default=None, db_nullable=True)
    infer_uuid: UUID | None = Field(default=None, db_nullable=True)
    infer_decimal: Decimal | None = Field(default=None, db_nullable=True)
    infer_json: dict | None = Field(default=None, db_nullable=True)
    infer_enum: Status = Field(default=Status.DRAFT)

    # Explicit db_type (scalar)
    db_uuid: str = Field(default="", db_type="UUID")
    db_jsonb: str = Field(default="", db_type="JSONB")
    db_json: str = Field(default="", db_type="JSON")
    db_varchar: str = Field(default="", db_type="VARCHAR(255)")
    db_char: str = Field(default="", db_type="CHAR(36)")
    db_text: str = Field(default="", db_type="TEXT")
    db_numeric: Decimal | None = Field(
        default=None, db_nullable=True, db_type="NUMERIC(10,2)"
    )
    db_decimal: Decimal | None = Field(
        default=None, db_nullable=True, db_type="DECIMAL(8,4)"
    )
    db_timestamp: datetime | None = Field(
        default=None, db_nullable=True, db_type="TIMESTAMP"
    )
    db_timestamptz: datetime | None = Field(
        default=None, db_nullable=True, db_type="TIMESTAMPTZ"
    )
    db_date: date | None = Field(default=None, db_nullable=True, db_type="DATE")
    db_time: time | None = Field(default=None, db_nullable=True, db_type="TIME")
    db_bigint: int = Field(default=0, db_type="BIGINT")
    db_integer: int = Field(default=0, db_type="INTEGER")
    db_smallint: int = Field(default=0, db_type="SMALLINT")
    db_serial: int | None = Field(default=None, db_pk=False, db_type="SERIAL")
    db_bigserial: int | None = Field(default=None, db_pk=False, db_type="BIGSERIAL")
    db_boolean: bool = Field(default=True, db_type="BOOLEAN")
    db_double: float = Field(default=0.0, db_type="DOUBLE PRECISION")
    db_real: float = Field(default=0.0, db_type="REAL")
    db_bytea: bytes | None = Field(default=None, db_nullable=True, db_type="BYTEA")
    db_blob: bytes | None = Field(default=None, db_nullable=True, db_type="BLOB")
    db_enum: Status = Field(default=Status.DRAFT, db_type="post_status_enum")
    db_enum_as_text: Status = Field(default=Status.DRAFT, db_type="TEXT")

    # Inferred array types (no db_type)
    infer_str_list: list[str] | None = Field(default=None, db_nullable=True)
    infer_int_list: list[int] | None = Field(default=None, db_nullable=True)
    infer_uuid_list: list[UUID] | None = Field(default=None, db_nullable=True)
    infer_decimal_list: list[Decimal] | None = Field(default=None, db_nullable=True)
    infer_enum_list: list[Status] | None = Field(default=None, db_nullable=True)

    # Explicit db_type on arrays
    db_varchar_arr: list[str] | None = Field(
        default=None, db_nullable=True, db_type="VARCHAR(100)[]"
    )
    db_numeric_arr: list[Decimal] | None = Field(
        default=None, db_nullable=True, db_type="NUMERIC(10,2)[]"
    )
    db_uuid_arr: list[UUID] | None = Field(
        default=None, db_nullable=True, db_type="UUID[]"
    )
    db_int_arr: list[int] | None = Field(
        default=None, db_nullable=True, db_type="INTEGER[]"
    )
    db_text_arr: list[str] | None = Field(
        default=None, db_nullable=True, db_type="TEXT[]"
    )
    db_enum_arr: list[Status] | None = Field(
        default=None, db_nullable=True, db_type="post_status_enum[]"
    )

    # Annotated inner constraints on arrays
    ann_str_list: list[Annotated[str, Field(max_length=100)]] | None = Field(
        default=None, db_nullable=True
    )
    ann_decimal_list: list[
        Annotated[Decimal, Field(max_digits=10, decimal_places=2)]
    ] | None = Field(default=None, db_nullable=True)

    class Meta:
        is_table = True
        table_name = "db_types_model"


# ── column_types tests ────────────────────────────────────────────────

COL_TYPE_CASES = [
    # Inferred scalar
    ("infer_str", {"kind": "string"}),
    ("infer_int", {"kind": "big_integer"}),
    ("infer_float", {"kind": "double"}),
    ("infer_bool", {"kind": "boolean"}),
    ("infer_bytes", {"kind": "blob"}),
    ("infer_datetime", {"kind": "date_time"}),
    ("infer_date", {"kind": "date"}),
    ("infer_time", {"kind": "time"}),
    ("infer_timedelta", {"kind": "timedelta"}),
    ("infer_uuid", {"kind": "uuid"}),
    ("infer_decimal", {"kind": "decimal"}),
    ("infer_json", {"kind": "json"}),
    (
        "infer_enum",
        {"kind": "enum", "name": "status_enum", "values": ["draft", "published"]},
    ),
    # Explicit db_type scalar — semantic kind via KNOWN_DB_TYPES,
    # the verbatim string travels separately (FieldDef.db_type)
    ("db_uuid", {"kind": "uuid"}),
    ("db_jsonb", {"kind": "json_binary"}),
    ("db_json", {"kind": "json"}),
    ("db_varchar", {"kind": "string", "length": 255}),
    ("db_char", {"kind": "string", "length": 36}),
    ("db_text", {"kind": "text"}),
    ("db_numeric", {"kind": "decimal", "precision": 10, "scale": 2}),
    ("db_decimal", {"kind": "decimal", "precision": 8, "scale": 4}),
    ("db_timestamp", {"kind": "date_time"}),
    ("db_timestamptz", {"kind": "date_time_utc"}),
    ("db_date", {"kind": "date"}),
    ("db_time", {"kind": "time"}),
    ("db_bigint", {"kind": "big_integer"}),
    ("db_integer", {"kind": "big_integer"}),
    ("db_smallint", {"kind": "big_integer"}),
    ("db_serial", {"kind": "big_integer"}),
    ("db_bigserial", {"kind": "big_integer"}),
    ("db_boolean", {"kind": "boolean"}),
    ("db_double", {"kind": "double"}),
    ("db_real", {"kind": "double"}),
    ("db_bytea", {"kind": "blob"}),
    ("db_blob", {"kind": "blob"}),
    (
        "db_enum",
        {
            "kind": "enum",
            "name": "post_status_enum",
            "values": ["draft", "published"],
        },
    ),
    ("db_enum_as_text", {"kind": "text"}),
    # Inferred arrays
    ("infer_str_list", {"kind": "array", "item": {"kind": "string"}}),
    ("infer_int_list", {"kind": "array", "item": {"kind": "big_integer"}}),
    ("infer_uuid_list", {"kind": "array", "item": {"kind": "uuid"}}),
    ("infer_decimal_list", {"kind": "array", "item": {"kind": "decimal"}}),
    (
        "infer_enum_list",
        {
            "kind": "array",
            "item": {
                "kind": "enum",
                "name": "status_enum",
                "values": ["draft", "published"],
            },
        },
    ),
    # Explicit db_type arrays — kind per element, params parsed
    ("db_varchar_arr", {"kind": "array", "item": {"kind": "string", "length": 100}}),
    (
        "db_numeric_arr",
        {"kind": "array", "item": {"kind": "decimal", "precision": 10, "scale": 2}},
    ),
    ("db_uuid_arr", {"kind": "array", "item": {"kind": "uuid"}}),
    ("db_int_arr", {"kind": "array", "item": {"kind": "big_integer"}}),
    ("db_text_arr", {"kind": "array", "item": {"kind": "text"}}),
    (
        "db_enum_arr",
        {
            "kind": "array",
            "item": {
                "kind": "enum",
                "name": "post_status_enum",
                "values": ["draft", "published"],
            },
        },
    ),
    # Annotated inner arrays (inferred from python_type)
    ("ann_str_list", {"kind": "array", "item": {"kind": "string", "length": 100}}),
    ("ann_decimal_list", {"kind": "array", "item": {"kind": "decimal", "precision": 10, "scale": 2}}),
]


class TestColTypes:
    @pytest.mark.parametrize("field,expected", COL_TYPE_CASES)
    def test_col_type(self, field, expected):
        column_types = DbTypesModel._db_meta.column_types
        assert column_types[field] == expected, (
            f"{field}: got {column_types.get(field)!r}, expected {expected!r}"
        )

    def test_int_enum_error_is_actionable(self):
        clear_registry()
        try:
            with pytest.raises(TypeError, match="Use a str-valued Enum"):

                class Task(Model):
                    id: int | None = Field(default=None, db_pk=True)
                    priority: Priority = Field(default=Priority.LOW)

                    class Meta:
                        is_table = True
                        table_name = "int_enum_tasks"
        finally:
            # The failed Task stays in _PENDING_MODELS otherwise and re-raises
            # on the next model finalization in an unrelated test.
            clear_registry()


# ── Annotated inner constraints extraction ────────────────────────────


class TestAnnotatedConstraints:
    def test_str_list_max_length(self):
        meta = DbTypesModel._db_meta.field_metadata["ann_str_list"]
        assert meta.max_length == 100

    def test_decimal_list_max_digits(self):
        meta = DbTypesModel._db_meta.field_metadata["ann_decimal_list"]
        assert meta.max_digits == 10
        assert meta.decimal_places == 2

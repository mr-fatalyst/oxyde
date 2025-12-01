"""Tests for migration schema change detection."""

from __future__ import annotations

from datetime import date, datetime, time
from uuid import UUID

import pytest

from oxyde import Field, OxydeModel
from oxyde.models.registry import clear_registry, registered_tables


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


class TestSchemaExtraction:
    """Test schema extraction from models."""

    def test_extract_basic_model_schema(self):
        """Test extracting schema from basic model."""

        class SimpleModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            value: int = 0

            class Meta:
                is_table = True

        registered_tables()
        meta = SimpleModel._db_meta.field_metadata

        assert "id" in meta
        assert "name" in meta
        assert "value" in meta

        assert meta["id"].primary_key is True
        assert meta["name"].nullable is False
        assert meta["value"].default == 0

    def test_extract_nullable_fields(self):
        """Test extracting nullable field information."""

        class NullableModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            required: str
            optional: str | None = None

            class Meta:
                is_table = True

        registered_tables()
        meta = NullableModel._db_meta.field_metadata

        assert meta["required"].nullable is False
        assert meta["optional"].nullable is True

    def test_extract_indexed_fields(self):
        """Test extracting index information."""

        class IndexedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            indexed_field: str = Field(db_index=True)
            unique_field: str = Field(db_unique=True)

            class Meta:
                is_table = True

        registered_tables()
        meta = IndexedModel._db_meta.field_metadata

        assert meta["indexed_field"].index is True
        assert meta["unique_field"].unique is True

    def test_extract_foreign_key_fields(self):
        """Test extracting FK information."""

        class Parent(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        class Child(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            parent: Parent = Field(db_on_delete="CASCADE")

            class Meta:
                is_table = True

        registered_tables()
        meta = Child._db_meta.field_metadata

        assert meta["parent"].foreign_key is not None
        assert meta["parent"].foreign_key.on_delete == "CASCADE"

    def test_extract_table_level_indexes(self):
        """Test extracting table-level indexes from Meta."""
        from oxyde.models.decorators import Index

        class IndexedTable(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            first_name: str
            last_name: str

            class Meta:
                is_table = True
                indexes = [
                    Index(fields=["first_name", "last_name"], name="idx_full_name"),
                ]

        assert len(IndexedTable._db_meta.indexes) == 1
        assert IndexedTable._db_meta.indexes[0].name == "idx_full_name"

    def test_extract_unique_together(self):
        """Test extracting unique_together constraints."""

        class UniqueTogetherModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            tenant_id: int
            code: str

            class Meta:
                is_table = True
                unique_together = [("tenant_id", "code")]

        assert len(UniqueTogetherModel._db_meta.unique_together) == 1
        assert UniqueTogetherModel._db_meta.unique_together[0] == ("tenant_id", "code")


class TestFieldTypeDetection:
    """Test field type detection for migrations."""

    def test_detect_string_type(self):
        """Test string type detection."""

        class StringModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        registered_tables()
        meta = StringModel._db_meta.field_metadata

        assert meta["name"].python_type == str

    def test_detect_varchar_with_length(self):
        """Test varchar with max_length detection."""

        class VarcharModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            title: str = Field(max_length=100)

            class Meta:
                is_table = True

        registered_tables()
        meta = VarcharModel._db_meta.field_metadata

        assert meta["title"].max_length == 100

    def test_detect_integer_types(self):
        """Test integer type detection."""

        class IntModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            count: int
            big_count: int = Field(db_type="BIGINT")

            class Meta:
                is_table = True

        registered_tables()
        meta = IntModel._db_meta.field_metadata

        assert meta["count"].python_type == int
        assert meta["big_count"].db_type == "BIGINT"

    def test_detect_float_types(self):
        """Test float type detection."""

        class FloatModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            price: float
            precise: float = Field(db_type="NUMERIC(10,2)")

            class Meta:
                is_table = True

        registered_tables()
        meta = FloatModel._db_meta.field_metadata

        assert meta["price"].python_type == float
        assert meta["precise"].db_type == "NUMERIC(10,2)"

    def test_detect_datetime_types(self):
        """Test datetime type detection."""

        class DateTimeModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            created_at: datetime
            birth_date: date
            start_time: time

            class Meta:
                is_table = True

        registered_tables()
        meta = DateTimeModel._db_meta.field_metadata

        assert meta["created_at"].python_type == datetime
        assert meta["birth_date"].python_type == date
        assert meta["start_time"].python_type == time

    def test_detect_boolean_type(self):
        """Test boolean type detection."""

        class BoolModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            is_active: bool = True

            class Meta:
                is_table = True

        registered_tables()
        meta = BoolModel._db_meta.field_metadata

        assert meta["is_active"].python_type == bool

    def test_detect_uuid_type(self):
        """Test UUID type detection."""

        class UUIDModel(OxydeModel):
            id: UUID | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        registered_tables()
        meta = UUIDModel._db_meta.field_metadata

        assert meta["id"].python_type == UUID


class TestSchemaDiff:
    """Test schema difference detection."""

    def test_detect_new_field(self):
        """Test detecting new field addition."""
        # This is a conceptual test - actual implementation would compare
        # current model schema with database schema

        class OriginalModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        class ExtendedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            email: str | None = None  # New field

            class Meta:
                is_table = True
                table_name = "originalmodel"

        registered_tables()

        # Compare field sets
        original_fields = set(OriginalModel._db_meta.field_metadata.keys())
        extended_fields = set(ExtendedModel._db_meta.field_metadata.keys())

        new_fields = extended_fields - original_fields
        assert "email" in new_fields

    def test_detect_removed_field(self):
        """Test detecting field removal."""

        class FullModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            legacy_field: str = ""

            class Meta:
                is_table = True

        class ReducedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            # legacy_field removed

            class Meta:
                is_table = True
                table_name = "fullmodel"

        registered_tables()

        full_fields = set(FullModel._db_meta.field_metadata.keys())
        reduced_fields = set(ReducedModel._db_meta.field_metadata.keys())

        removed_fields = full_fields - reduced_fields
        assert "legacy_field" in removed_fields

    def test_detect_field_type_change(self):
        """Test detecting field type change."""

        class OldTypeModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            value: int

            class Meta:
                is_table = True

        class NewTypeModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            value: str  # Changed from int to str

            class Meta:
                is_table = True
                table_name = "oldtypemodel"

        registered_tables()

        old_meta = OldTypeModel._db_meta.field_metadata["value"]
        new_meta = NewTypeModel._db_meta.field_metadata["value"]

        assert old_meta.python_type != new_meta.python_type

    def test_detect_nullable_change(self):
        """Test detecting nullable change."""

        class RequiredModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            email: str

            class Meta:
                is_table = True

        class OptionalModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            email: str | None = None  # Now optional

            class Meta:
                is_table = True
                table_name = "requiredmodel"

        registered_tables()

        required_meta = RequiredModel._db_meta.field_metadata["email"]
        optional_meta = OptionalModel._db_meta.field_metadata["email"]

        assert required_meta.nullable != optional_meta.nullable

    def test_detect_index_addition(self):
        """Test detecting index addition."""

        class UnindexedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            email: str

            class Meta:
                is_table = True

        class IndexedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            email: str = Field(db_index=True)  # Index added

            class Meta:
                is_table = True
                table_name = "unindexedmodel"

        registered_tables()

        unindexed_meta = UnindexedModel._db_meta.field_metadata["email"]
        indexed_meta = IndexedModel._db_meta.field_metadata["email"]

        assert unindexed_meta.index != indexed_meta.index


class TestModelMetaOptions:
    """Test model Meta options relevant to migrations."""

    def test_custom_table_name(self):
        """Test custom table name."""

        class CustomTableModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True
                table_name = "custom_table"

        assert CustomTableModel.get_table_name() == "custom_table"

    def test_schema_option(self):
        """Test schema option."""

        class SchemaModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True
                schema = "public"

        assert SchemaModel._db_meta.schema == "public"

    def test_comment_option(self):
        """Test comment option."""

        class CommentedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True
                comment = "This is a test table"

        assert CommentedModel._db_meta.comment == "This is a test table"


class TestConstraintDetection:
    """Test constraint detection for migrations."""

    def test_detect_check_constraint(self):
        """Test detecting check constraints."""
        from oxyde.models.decorators import Check

        class CheckModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            age: int

            class Meta:
                is_table = True
                constraints = [
                    Check("age >= 0", name="age_positive"),
                ]

        assert len(CheckModel._db_meta.constraints) == 1
        assert CheckModel._db_meta.constraints[0].name == "age_positive"

    def test_detect_primary_key(self):
        """Test detecting primary key."""

        class PKModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        registered_tables()
        meta = PKModel._db_meta.field_metadata

        assert meta["id"].primary_key is True

    def test_detect_unique_constraint(self):
        """Test detecting unique constraint."""

        class UniqueModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            code: str = Field(db_unique=True)

            class Meta:
                is_table = True

        registered_tables()
        meta = UniqueModel._db_meta.field_metadata

        assert meta["code"].unique is True

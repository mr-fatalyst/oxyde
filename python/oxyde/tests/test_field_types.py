"""Tests for OxydeFieldInfo and Field() function with all parameter combinations."""

from __future__ import annotations

from datetime import date, datetime, time
from decimal import Decimal
from uuid import UUID

import pytest

from oxyde import Field, OxydeModel
from oxyde.models.field import OxydeFieldInfo
from oxyde.models.registry import clear_registry, registered_tables


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


class TestOxydeFieldInfoInit:
    """Test OxydeFieldInfo initialization with various parameters."""

    def test_default_values(self):
        """Test OxydeFieldInfo with default values."""
        field = OxydeFieldInfo()
        assert field.db_pk is False
        assert field.db_index is False
        assert field.db_index_name is None
        assert field.db_index_method is None
        assert field.db_unique is False
        assert field.db_column is None
        assert field.db_type is None
        assert field.db_default is None
        assert field.db_comment is None
        assert field.db_on_delete == "RESTRICT"
        assert field.db_on_update == "CASCADE"
        assert field.db_reverse_fk is None
        assert field.db_m2m is False
        assert field.db_through is None

    def test_primary_key(self):
        """Test OxydeFieldInfo with db_pk=True."""
        field = OxydeFieldInfo(db_pk=True)
        assert field.db_pk is True

    def test_index_parameters(self):
        """Test OxydeFieldInfo with index parameters."""
        field = OxydeFieldInfo(
            db_index=True,
            db_index_name="idx_custom",
            db_index_method="btree",
        )
        assert field.db_index is True
        assert field.db_index_name == "idx_custom"
        assert field.db_index_method == "btree"

    def test_unique_constraint(self):
        """Test OxydeFieldInfo with db_unique=True."""
        field = OxydeFieldInfo(db_unique=True)
        assert field.db_unique is True

    def test_column_name_override(self):
        """Test OxydeFieldInfo with db_column override."""
        field = OxydeFieldInfo(db_column="custom_column")
        assert field.db_column == "custom_column"

    def test_sql_type_override(self):
        """Test OxydeFieldInfo with db_type override."""
        field = OxydeFieldInfo(db_type="JSONB")
        assert field.db_type == "JSONB"

    def test_db_default_expression(self):
        """Test OxydeFieldInfo with db_default SQL expression."""
        field = OxydeFieldInfo(db_default="NOW()")
        assert field.db_default == "NOW()"

    def test_db_comment(self):
        """Test OxydeFieldInfo with db_comment."""
        field = OxydeFieldInfo(db_comment="User email address")
        assert field.db_comment == "User email address"

    def test_foreign_key_actions(self):
        """Test OxydeFieldInfo with FK actions."""
        field = OxydeFieldInfo(db_on_delete="CASCADE", db_on_update="SET NULL")
        assert field.db_on_delete == "CASCADE"
        assert field.db_on_update == "SET NULL"

    def test_reverse_fk(self):
        """Test OxydeFieldInfo with db_reverse_fk."""
        field = OxydeFieldInfo(db_reverse_fk="posts")
        assert field.db_reverse_fk == "posts"

    def test_m2m_parameters(self):
        """Test OxydeFieldInfo with M2M parameters."""
        field = OxydeFieldInfo(db_m2m=True, db_through="PostTag")
        assert field.db_m2m is True
        assert field.db_through == "PostTag"

    def test_all_index_methods(self):
        """Test all supported index methods."""
        for method in ["btree", "hash", "gin", "gist"]:
            field = OxydeFieldInfo(db_index=True, db_index_method=method)
            assert field.db_index_method == method

    def test_all_on_delete_actions(self):
        """Test all supported ON DELETE actions."""
        for action in ["CASCADE", "SET NULL", "RESTRICT", "NO ACTION"]:
            field = OxydeFieldInfo(db_on_delete=action)
            assert field.db_on_delete == action

    def test_all_on_update_actions(self):
        """Test all supported ON UPDATE actions."""
        for action in ["CASCADE", "SET NULL", "RESTRICT", "NO ACTION"]:
            field = OxydeFieldInfo(db_on_update=action)
            assert field.db_on_update == action

    def test_pydantic_parameters_passthrough(self):
        """Test that Pydantic parameters are passed through."""
        field = OxydeFieldInfo(
            default="test",
            ge=0,
            le=100,
            description="Test field",
            alias="test_alias",
        )
        assert field.default == "test"
        assert field.description == "Test field"
        assert field.alias == "test_alias"


class TestFieldFactory:
    """Test Field() factory function."""

    def test_field_returns_oxyde_field_info(self):
        """Test that Field() returns OxydeFieldInfo instance."""
        field = Field()
        assert isinstance(field, OxydeFieldInfo)

    def test_field_with_default(self):
        """Test Field() with default value."""
        field = Field(default=42)
        assert field.default == 42

    def test_field_with_db_parameters(self):
        """Test Field() with database parameters."""
        field = Field(db_pk=True, db_index=True, db_unique=True)
        assert field.db_pk is True
        assert field.db_index is True
        assert field.db_unique is True

    def test_field_with_mixed_parameters(self):
        """Test Field() with mixed Pydantic and DB parameters."""
        field = Field(
            default=0,
            ge=0,
            le=100,
            db_index=True,
            db_comment="Rating value",
        )
        assert field.default == 0
        assert field.db_index is True
        assert field.db_comment == "Rating value"


class TestModelFieldMetadata:
    """Test field metadata extraction in OxydeModel."""

    def test_basic_field_types(self):
        """Test metadata extraction for basic field types."""

        class BasicModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            age: int
            price: float
            is_active: bool = True
            balance: Decimal = Decimal("0.00")

            class Meta:
                is_table = True

        registered_tables()  # Trigger metadata parsing

        meta = BasicModel._db_meta.field_metadata

        assert meta["id"].primary_key is True
        assert meta["name"].python_type == str
        assert meta["age"].python_type == int
        assert meta["price"].python_type == float
        assert meta["is_active"].python_type == bool
        assert meta["balance"].python_type == Decimal

    def test_datetime_field_types(self):
        """Test metadata extraction for datetime types."""

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

    def test_optional_fields(self):
        """Test metadata extraction for optional fields."""

        class OptionalModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str | None = None
            age: int | None = None
            email: str | None = Field(default=None, db_index=True)

            class Meta:
                is_table = True

        registered_tables()

        meta = OptionalModel._db_meta.field_metadata

        assert meta["name"].nullable is True
        assert meta["age"].nullable is True
        assert meta["email"].nullable is True
        assert meta["email"].index is True

    def test_field_with_max_length(self):
        """Test metadata extraction for fields with max_length."""

        class LengthModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            title: str = Field(max_length=200)
            description: str = Field(max_length=1000)

            class Meta:
                is_table = True

        registered_tables()

        meta = LengthModel._db_meta.field_metadata

        assert meta["title"].max_length == 200
        assert meta["description"].max_length == 1000

    def test_field_with_db_column_override(self):
        """Test metadata extraction with db_column override."""

        class ColumnModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            created_at: datetime = Field(db_column="created_timestamp")

            class Meta:
                is_table = True

        registered_tables()

        meta = ColumnModel._db_meta.field_metadata

        assert meta["created_at"].db_column == "created_timestamp"

    def test_field_with_db_type_override(self):
        """Test metadata extraction with db_type override."""

        class TypeModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True, db_type="BIGSERIAL")
            data: str = Field(db_type="JSONB")

            class Meta:
                is_table = True

        registered_tables()

        meta = TypeModel._db_meta.field_metadata

        assert meta["id"].db_type == "BIGSERIAL"
        assert meta["data"].db_type == "JSONB"

    def test_field_with_db_default(self):
        """Test metadata extraction with db_default."""

        class DefaultModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            created_at: datetime | None = Field(default=None, db_default="NOW()")
            counter: int = Field(default=0, db_default="0")

            class Meta:
                is_table = True

        registered_tables()

        meta = DefaultModel._db_meta.field_metadata

        assert meta["created_at"].db_default == "NOW()"
        assert meta["counter"].db_default == "0"

    def test_field_with_comment(self):
        """Test metadata extraction with db_comment."""

        class CommentModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            email: str = Field(db_comment="User email address")

            class Meta:
                is_table = True

        registered_tables()

        meta = CommentModel._db_meta.field_metadata

        assert meta["email"].comment == "User email address"

    def test_index_and_unique_fields(self):
        """Test metadata extraction for indexed and unique fields."""

        class IndexModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            email: str = Field(db_unique=True)
            category: str = Field(db_index=True)
            slug: str = Field(db_unique=True, db_index=True)

            class Meta:
                is_table = True

        registered_tables()

        meta = IndexModel._db_meta.field_metadata

        assert meta["email"].unique is True
        assert meta["category"].index is True
        assert meta["slug"].unique is True
        assert meta["slug"].index is True

    def test_uuid_field(self):
        """Test metadata extraction for UUID field."""

        class UUIDModel(OxydeModel):
            id: UUID | None = Field(default=None, db_pk=True)
            external_id: UUID | None = None

            class Meta:
                is_table = True

        registered_tables()

        meta = UUIDModel._db_meta.field_metadata

        assert meta["id"].python_type == UUID
        assert meta["external_id"].python_type == UUID

    def test_combined_field_attributes(self):
        """Test field with multiple attributes combined."""

        class CombinedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            email: str = Field(
                max_length=255,
                db_unique=True,
                db_index=True,
                db_comment="Primary email",
            )

            class Meta:
                is_table = True

        registered_tables()

        meta = CombinedModel._db_meta.field_metadata

        assert meta["email"].max_length == 255
        assert meta["email"].unique is True
        assert meta["email"].index is True
        assert meta["email"].comment == "Primary email"


class TestFieldDescriptorAccess:
    """Test field descriptor access patterns."""

    def test_instance_attribute_returns_value(self):
        """Test that instance attribute access returns value."""

        class TestModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        instance = TestModel(id=1, name="Test")
        assert instance.name == "Test"
        assert instance.id == 1


class TestModelSerialization:
    """Test model serialization methods."""

    def test_model_to_dict(self):
        """Test model.model_dump() method."""

        class SerModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            age: int

            class Meta:
                is_table = True

        instance = SerModel(id=1, name="Alice", age=30)
        data = instance.model_dump()

        assert data == {"id": 1, "name": "Alice", "age": 30}

    def test_model_from_dict(self):
        """Test model creation from dict."""

        class SerModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            age: int

            class Meta:
                is_table = True

        data = {"id": 1, "name": "Bob", "age": 25}
        instance = SerModel.model_validate(data)

        assert instance.id == 1
        assert instance.name == "Bob"
        assert instance.age == 25

    def test_model_with_defaults(self):
        """Test model with default values."""

        class DefaultsModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            status: str = "active"
            count: int = 0

            class Meta:
                is_table = True

        instance = DefaultsModel(id=1, name="Test")

        assert instance.status == "active"
        assert instance.count == 0

    def test_model_json_serialization(self):
        """Test model JSON serialization."""

        class JsonModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            created_at: datetime

            class Meta:
                is_table = True

        now = datetime(2024, 1, 15, 12, 30, 45)
        instance = JsonModel(id=1, name="Test", created_at=now)

        json_data = instance.model_dump(mode="json")
        assert json_data["created_at"] == "2024-01-15T12:30:45"


class TestFieldValidation:
    """Test Pydantic field validation."""

    def test_ge_le_validators(self):
        """Test ge/le validators."""

        class ValidatedModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            age: int = Field(ge=0, le=150)
            rating: float = Field(ge=0.0, le=5.0)

            class Meta:
                is_table = True

        # Valid values
        instance = ValidatedModel(id=1, age=25, rating=4.5)
        assert instance.age == 25

        # Invalid values
        with pytest.raises(Exception):  # Pydantic ValidationError
            ValidatedModel(id=1, age=-1, rating=4.5)

        with pytest.raises(Exception):
            ValidatedModel(id=1, age=25, rating=6.0)

    def test_string_length_validators(self):
        """Test string length validators."""

        class LengthModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            code: str = Field(min_length=2, max_length=10)

            class Meta:
                is_table = True

        # Valid value
        instance = LengthModel(id=1, code="ABC")
        assert instance.code == "ABC"

        # Too short
        with pytest.raises(Exception):
            LengthModel(id=1, code="A")

        # Too long
        with pytest.raises(Exception):
            LengthModel(id=1, code="A" * 20)

"""Tests for model inheritance scenarios."""

from __future__ import annotations

import pytest

from oxyde import Field, Model
from oxyde.models.registry import clear_registry, registered_tables


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


class TestBasicInheritance:
    """Test basic model inheritance patterns."""

    def test_non_table_subclass_not_registered(self):
        """Test that non-table subclass is not registered."""

        class BaseUser(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str

            class Meta:
                is_table = True

        class UserCreate(BaseUser):
            """DTO for creating users - not a table."""

            pass

        tables = registered_tables()

        base_key = f"{BaseUser.__module__}.{BaseUser.__qualname__}"
        create_key = f"{UserCreate.__module__}.{UserCreate.__qualname__}"

        assert base_key in tables
        assert create_key not in tables

    def test_explicit_non_table_subclass(self):
        """Test subclass with explicit is_table=False."""

        class BaseModel(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        class ResponseModel(BaseModel):
            class Meta:
                is_table = False

        assert BaseModel._is_table is True
        assert ResponseModel._is_table is False

    def test_subclass_inherits_fields(self):
        """Test that subclass inherits parent fields."""

        class Animal(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            age: int

            class Meta:
                is_table = True

        class Dog(Animal):
            breed: str

            class Meta:
                is_table = True

        dog = Dog(id=1, name="Rex", age=3, breed="Labrador")

        assert dog.name == "Rex"
        assert dog.age == 3
        assert dog.breed == "Labrador"

    def test_subclass_can_override_defaults(self):
        """Test that subclass can override default values."""

        class BaseEntity(Model):
            id: int | None = Field(default=None, db_pk=True)
            status: str = "pending"

            class Meta:
                is_table = True

        class ApprovedEntity(BaseEntity):
            status: str = "approved"

            class Meta:
                is_table = True

        entity = ApprovedEntity(id=1)
        assert entity.status == "approved"


class TestMetaInheritance:
    """Test Meta class inheritance."""

    def test_meta_table_name_inheritance(self):
        """Test table name is specific to each model."""

        class Person(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True
                table_name = "persons"

        class Employee(Person):
            department: str

            class Meta:
                is_table = True
                table_name = "employees"

        assert Person.get_table_name() == "persons"
        assert Employee.get_table_name() == "employees"

    def test_meta_default_table_name(self):
        """Test default table name is lowercase class name."""

        class MyModel(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        assert MyModel.get_table_name() == "mymodel"

    def test_meta_schema_inheritance(self):
        """Test schema configuration."""

        class SchemaModel(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True
                schema = "public"

        assert SchemaModel._db_meta.schema == "public"


class TestFieldMetadataInheritance:
    """Test field metadata inheritance."""

    def test_parent_field_metadata_preserved(self):
        """Test that parent field metadata is preserved in subclass."""

        class BaseRecord(Model):
            id: int | None = Field(default=None, db_pk=True)
            created_at: str = Field(db_index=True)

            class Meta:
                is_table = True

        class ExtendedRecord(BaseRecord):
            extra_field: str

            class Meta:
                is_table = True

        registered_tables()

        base_meta = BaseRecord._db_meta.field_metadata
        extended_meta = ExtendedRecord._db_meta.field_metadata

        assert base_meta["created_at"].index is True
        assert extended_meta["created_at"].index is True

    def test_subclass_adds_new_fields(self):
        """Test that subclass can add new fields."""

        class BaseItem(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        class DetailedItem(BaseItem):
            description: str
            price: float

            class Meta:
                is_table = True

        registered_tables()

        meta = DetailedItem._db_meta.field_metadata

        assert "id" in meta
        assert "name" in meta
        assert "description" in meta
        assert "price" in meta


class TestManagerInheritance:
    """Test that each model gets its own manager."""

    def test_each_model_has_own_manager(self):
        """Test that each model class has its own manager instance."""

        class ModelA(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        class ModelB(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        assert ModelA.objects is not ModelB.objects
        assert ModelA.objects.model_class is ModelA
        assert ModelB.objects.model_class is ModelB

    def test_subclass_has_own_manager(self):
        """Test that subclass has its own manager."""

        class Parent(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        class Child(Parent):
            extra: str = ""

            class Meta:
                is_table = True

        assert Parent.objects is not Child.objects
        assert Child.objects.model_class is Child


class TestAbstractModels:
    """Test abstract-like model patterns."""

    def test_non_table_base_class(self):
        """Test using non-table base class for shared fields."""

        class TimestampMixin(Model):
            created_at: str = Field(default="", db_index=True)
            updated_at: str = Field(default="")

        class Article(TimestampMixin):
            id: int | None = Field(default=None, db_pk=True)
            title: str

            class Meta:
                is_table = True

        class Comment(TimestampMixin):
            id: int | None = Field(default=None, db_pk=True)
            body: str

            class Meta:
                is_table = True

        tables = registered_tables()

        # Mixin should not be registered
        mixin_key = f"{TimestampMixin.__module__}.{TimestampMixin.__qualname__}"
        assert mixin_key not in tables

        # Concrete classes should be registered
        article_key = f"{Article.__module__}.{Article.__qualname__}"
        comment_key = f"{Comment.__module__}.{Comment.__qualname__}"
        assert article_key in tables
        assert comment_key in tables

        # Both should have timestamp fields
        registered_tables()
        assert "created_at" in Article._db_meta.field_metadata
        assert "created_at" in Comment._db_meta.field_metadata


class TestInstanceCreation:
    """Test instance creation with inheritance."""

    def test_create_parent_instance(self):
        """Test creating parent model instance."""

        class Vehicle(Model):
            id: int | None = Field(default=None, db_pk=True)
            brand: str
            year: int

            class Meta:
                is_table = True

        vehicle = Vehicle(id=1, brand="Toyota", year=2020)
        assert isinstance(vehicle, Vehicle)
        assert vehicle.brand == "Toyota"

    def test_create_child_instance(self):
        """Test creating child model instance."""

        class Vehicle(Model):
            id: int | None = Field(default=None, db_pk=True)
            brand: str
            year: int

            class Meta:
                is_table = True

        class Car(Vehicle):
            doors: int = 4

            class Meta:
                is_table = True

        car = Car(id=1, brand="Honda", year=2021, doors=2)
        assert isinstance(car, Car)
        assert isinstance(car, Vehicle)
        assert car.brand == "Honda"
        assert car.doors == 2

    def test_child_instance_validation(self):
        """Test that child instance validates all fields."""

        class BaseProduct(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            price: float = Field(ge=0)

            class Meta:
                is_table = True

        class DigitalProduct(BaseProduct):
            download_url: str

            class Meta:
                is_table = True

        # Valid instance
        product = DigitalProduct(
            id=1, name="E-book", price=9.99, download_url="https://example.com/download"
        )
        assert product.price == 9.99

        # Invalid price (inherited validation)
        with pytest.raises(Exception):  # Pydantic ValidationError
            DigitalProduct(
                id=1,
                name="E-book",
                price=-1.0,
                download_url="https://example.com/download",
            )


class TestQueryInheritance:
    """Test query operations with inherited models."""

    def test_filter_on_inherited_field(self):
        """Test filtering on field inherited from parent."""

        class BaseLog(Model):
            id: int | None = Field(default=None, db_pk=True)
            level: str
            message: str

            class Meta:
                is_table = True

        class ErrorLog(BaseLog):
            stack_trace: str = ""

            class Meta:
                is_table = True

        # Should be able to filter on inherited 'level' field
        ir = ErrorLog.objects.filter(level="ERROR").to_ir()
        assert ir["filter_tree"]["field"] == "level"

    def test_filter_on_child_field(self):
        """Test filtering on field defined in child."""

        class BaseEvent(Model):
            id: int | None = Field(default=None, db_pk=True)
            timestamp: str

            class Meta:
                is_table = True

        class ClickEvent(BaseEvent):
            element_id: str

            class Meta:
                is_table = True

        ir = ClickEvent.objects.filter(element_id="btn-submit").to_ir()
        assert ir["filter_tree"]["field"] == "element_id"

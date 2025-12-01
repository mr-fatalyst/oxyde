"""Tests for relation descriptors: FK, reverse FK (db_reverse_fk), M2M (db_m2m)."""

from __future__ import annotations

import msgpack
import pytest

from oxyde import Field, OxydeModel
from oxyde.models.registry import clear_registry, registered_tables


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


class StubExecuteClient:
    """Stub client for testing - returns msgpack encoded data."""

    def __init__(self, payloads: list):
        self.payloads = list(payloads)
        self.calls: list = []

    async def execute(self, ir: dict) -> bytes:
        self.calls.append(ir)
        if not self.payloads:
            raise RuntimeError("stub payloads exhausted")
        payload = self.payloads.pop(0)
        if isinstance(payload, bytes):
            return payload
        return msgpack.packb(payload)


class TestForeignKeyDetection:
    """Test automatic FK detection from type hints."""

    def test_fk_detected_from_model_type(self):
        """Test FK is detected when field type is another OxydeModel."""

        class Author(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        class Book(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            title: str
            author: Author = Field()

            class Meta:
                is_table = True

        registered_tables()

        meta = Book._db_meta.field_metadata["author"]
        assert meta.foreign_key is not None
        assert meta.foreign_key.column_name == "author_id"

    def test_optional_fk_is_nullable(self):
        """Test optional FK field is nullable."""

        class Category(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        class Product(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            category: Category | None = None

            class Meta:
                is_table = True

        registered_tables()

        meta = Product._db_meta.field_metadata["category"]
        assert meta.foreign_key is not None
        assert meta.nullable is True

    def test_fk_column_name_can_be_overridden(self):
        """Test FK column name can be overridden with db_column."""

        class User(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        class Post(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            title: str
            creator: User = Field(db_column="created_by")

            class Meta:
                is_table = True

        registered_tables()

        meta = Post._db_meta.field_metadata["creator"]
        assert meta.foreign_key.column_name == "created_by"

    def test_fk_on_delete_action(self):
        """Test FK on_delete action configuration."""

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

        meta = Child._db_meta.field_metadata["parent"]
        assert meta.foreign_key.on_delete == "CASCADE"

    def test_fk_on_update_action(self):
        """Test FK on_update action configuration."""

        class Parent(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        class Child(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            parent: Parent = Field(db_on_update="SET NULL")

            class Meta:
                is_table = True

        registered_tables()

        meta = Child._db_meta.field_metadata["parent"]
        assert meta.foreign_key.on_update == "SET NULL"


class TestReverseFKRelation:
    """Test reverse FK relation via Field(db_reverse_fk=...)."""

    def test_reverse_fk_field_stores_metadata(self):
        """Test Field(db_reverse_fk=...) stores relation metadata."""

        class Comment(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            post_id: int = 0
            body: str = ""

            class Meta:
                is_table = True

        class Post(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            # No default_factory - should be added automatically
            comments: list[Comment] = Field(db_reverse_fk="post_id")

            class Meta:
                is_table = True

        field = Post.model_fields["comments"]
        assert field.db_reverse_fk == "post_id"

    def test_reverse_fk_with_string_target(self):
        """Test reverse FK with list type hint."""

        class Reply(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            message_id: int = 0

            class Meta:
                is_table = True

        class Message(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            text: str = ""
            # No default_factory - should be added automatically
            replies: list[Reply] = Field(db_reverse_fk="message_id")

            class Meta:
                is_table = True

        field = Message.model_fields["replies"]
        assert field.db_reverse_fk == "message_id"

    def test_reverse_fk_auto_default_factory(self):
        """Test that default_factory=list is added automatically for db_reverse_fk fields."""

        class Item(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            container_id: int = 0

            class Meta:
                is_table = True

        class Container(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            # No default or default_factory specified
            items: list[Item] = Field(db_reverse_fk="container_id")

            class Meta:
                is_table = True

        # Check default_factory was added
        field = Container.model_fields["items"]
        assert field.default_factory is list

        # Check instances are isolated (each has own list)
        c1 = Container()
        c2 = Container()
        assert c1.items == []
        assert c2.items == []
        c1.items.append(Item())
        assert len(c1.items) == 1
        assert len(c2.items) == 0  # c2 should not be affected


class TestManyToManyRelation:
    """Test M2M relation via Field(db_m2m=True, db_through=...)."""

    def test_m2m_field_stores_metadata(self):
        """Test Field(db_m2m=True, db_through=...) stores relation metadata."""

        class Tag(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class PostTag(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            post_id: int = 0
            tag_id: int = 0

            class Meta:
                is_table = True

        class Article(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            tags: list[Tag] = Field(db_m2m=True, db_through="PostTag")

            class Meta:
                is_table = True

        field = Article.model_fields["tags"]
        assert field.db_m2m is True
        assert field.db_through == "PostTag"

    def test_m2m_with_model_through(self):
        """Test M2M with model class as through parameter."""

        class Skill(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class EmployeeSkill(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            employee_id: int = 0
            skill_id: int = 0

            class Meta:
                is_table = True

        class Employee(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""
            skills: list[Skill] = Field(db_m2m=True, db_through="EmployeeSkill")

            class Meta:
                is_table = True

        field = Employee.model_fields["skills"]
        assert field.db_m2m is True
        assert field.db_through == "EmployeeSkill"


class TestJoinRelations:
    """Test join operations with relations."""

    def test_join_generates_join_spec(self):
        """Test that join() generates proper join specification."""

        class Writer(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            email: str = ""

            class Meta:
                is_table = True

        class Blog(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            writer: Writer | None = None

            class Meta:
                is_table = True

        ir = Blog.objects.join("writer").to_ir()

        assert "joins" in ir
        assert len(ir["joins"]) == 1
        assert ir["joins"][0]["path"] == "writer"

    @pytest.mark.asyncio
    async def test_join_hydrates_related_models(self):
        """Test that join properly hydrates related models."""

        class Creator(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            email: str = ""

            class Meta:
                is_table = True

        class Story(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            creator: Creator | None = None

            class Meta:
                is_table = True

        rows = [
            [
                {
                    "id": 1,
                    "title": "Story 1",
                    "creator": None,
                    "creator__id": 10,
                    "creator__email": "author@example.com",
                }
            ]
        ]

        stub = StubExecuteClient(rows)
        stories = await Story.objects.join("creator").fetch_models(stub)

        assert len(stories) == 1
        assert stories[0].creator is not None
        assert stories[0].creator.email == "author@example.com"


class TestPrefetchRelations:
    """Test prefetch operations with relations."""

    def test_prefetch_generates_prefetch_spec(self):
        """Test that prefetch() stores prefetch paths in QuerySet."""

        class Review(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            product_id: int = 0
            rating: int = 0

            class Meta:
                is_table = True

        class Product(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""
            reviews: list[Review] = Field(db_reverse_fk="product_id")

            class Meta:
                is_table = True

        query = Product.objects.prefetch("reviews")

        # Prefetch paths are stored on the QuerySet itself
        assert len(query._prefetch_paths) == 1
        assert "reviews" in query._prefetch_paths

    @pytest.mark.asyncio
    async def test_prefetch_populates_relation(self):
        """Test that prefetch properly populates relation."""

        class Note(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            task_id: int = 0
            text: str = ""

            class Meta:
                is_table = True

        class Task(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            notes: list[Note] = Field(db_reverse_fk="task_id")

            class Meta:
                is_table = True

        base_rows = [[{"id": 1, "title": "Task 1"}]]
        note_rows = [[{"id": 1, "task_id": 1, "text": "Note 1"}]]

        stub = StubExecuteClient(base_rows + note_rows)
        tasks = await Task.objects.prefetch("notes").fetch_models(stub)

        assert len(tasks) == 1
        assert len(tasks[0].notes) == 1
        assert tasks[0].notes[0].text == "Note 1"


class TestSelfReferentialRelations:
    """Test self-referential relations."""

    def test_self_referential_fk(self):
        """Test self-referential FK (e.g., parent-child)."""

        class TreeNode(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""
            parent_id: int | None = None

            class Meta:
                is_table = True

        registered_tables()

        meta = TreeNode._db_meta.field_metadata
        assert "parent_id" in meta

    def test_self_referential_with_reverse_fk(self):
        """Test self-referential with reverse FK."""

        class Category(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""
            parent_id: int | None = None
            children: list[Category] = Field(db_reverse_fk="parent_id")

            class Meta:
                is_table = True

        field = Category.model_fields["children"]
        assert field.db_reverse_fk == "parent_id"


class TestRelationValidation:
    """Test relation validation."""

    def test_join_requires_path(self):
        """Test that join() requires at least one path."""

        class SimpleModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        with pytest.raises(ValueError):
            SimpleModel.objects.join()

    def test_prefetch_requires_path(self):
        """Test that prefetch() requires at least one path."""

        class SimpleModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        with pytest.raises(ValueError):
            SimpleModel.objects.prefetch()


class TestDbFkParameter:
    """Test db_fk parameter for explicit FK target specification."""

    def test_db_fk_with_model_type_targets_pk_by_default(self):
        """Test FK to model type targets PK by default."""

        class Account(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Profile(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            account: Account = Field()

            class Meta:
                is_table = True

        registered_tables()

        meta = Profile._db_meta.field_metadata["account"]
        assert meta.foreign_key is not None
        assert meta.foreign_key.target_field == "id"
        assert meta.foreign_key.column_name == "account_id"

    def test_db_fk_with_model_type_targets_custom_field(self):
        """Test FK to model type can target non-PK field via db_fk."""

        class Tenant(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            uuid: str = Field(db_unique=True)

            class Meta:
                is_table = True

        class Resource(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            tenant: Tenant = Field(db_fk="uuid", db_on_delete="CASCADE")

            class Meta:
                is_table = True

        registered_tables()

        meta = Resource._db_meta.field_metadata["tenant"]
        assert meta.foreign_key is not None
        assert meta.foreign_key.target_field == "uuid"
        assert (
            meta.foreign_key.column_name == "tenant_uuid"
        )  # {field_name}_{target_field}
        assert meta.foreign_key.on_delete == "CASCADE"

    def test_db_fk_column_naming_with_uuid_pk(self):
        """Test FK column naming when target uses uuid as PK."""

        class Organization(OxydeModel):
            uuid: str = Field(db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Member(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            org: Organization = Field()  # No db_fk - should auto-detect uuid as PK

            class Meta:
                is_table = True

        registered_tables()

        meta = Member._db_meta.field_metadata["org"]
        assert meta.foreign_key is not None
        assert meta.foreign_key.target_field == "uuid"
        assert meta.foreign_key.column_name == "org_uuid"  # {field_name}_{pk_field}

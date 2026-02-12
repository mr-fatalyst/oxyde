"""Tests for relation descriptors: FK, reverse FK (db_reverse_fk), M2M (db_m2m)."""

from __future__ import annotations

import msgpack
import pytest

from oxyde import Field, Model
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
        """Test FK is detected when field type is another Model."""

        class Author(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        class Book(Model):
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

        class Category(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        class Product(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            category: Category | None = None

            class Meta:
                is_table = True

        registered_tables()

        meta = Product._db_meta.field_metadata["category"]
        assert meta.foreign_key is not None
        assert meta.nullable is True

    def test_db_nullable_overrides_type_hint(self):
        """Test db_nullable explicitly overrides nullable from type hint.

        Note: db_nullable applies to the FK column (author_id), not the virtual field (author).
        """

        class Author(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        class Article(Model):
            id: int | None = Field(default=None, db_pk=True)
            title: str
            # author_id column: NOT NULL in DB (db_nullable=False)
            # author field: optional in Python (| None for Pydantic)
            author: Author | None = Field(db_nullable=False, db_on_delete="CASCADE")

            class Meta:
                is_table = True

        registered_tables()

        meta = Article._db_meta.field_metadata["author"]
        assert meta.foreign_key is not None
        assert meta.foreign_key.nullable is False  # author_id column is NOT NULL
        assert meta.nullable is False  # ColumnMeta.nullable reflects DB column

    def test_db_nullable_on_regular_field(self):
        """Test db_nullable works on non-FK fields too."""

        class Item(Model):
            id: int | None = Field(default=None, db_pk=True)
            # str but db_nullable=True => NULL in DB
            name: str = Field(db_nullable=True)
            # str | None but db_nullable=False => NOT NULL in DB
            code: str | None = Field(db_nullable=False)

            class Meta:
                is_table = True

        registered_tables()

        name_meta = Item._db_meta.field_metadata["name"]
        assert name_meta.nullable is True  # db_nullable overrides required str

        code_meta = Item._db_meta.field_metadata["code"]
        assert code_meta.nullable is False  # db_nullable overrides optional

    def test_fk_column_name_can_be_overridden(self):
        """Test FK column name can be overridden with db_column."""

        class User(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        class Post(Model):
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

        class Parent(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        class Child(Model):
            id: int | None = Field(default=None, db_pk=True)
            parent: Parent = Field(db_on_delete="CASCADE")

            class Meta:
                is_table = True

        registered_tables()

        meta = Child._db_meta.field_metadata["parent"]
        assert meta.foreign_key.on_delete == "CASCADE"

    def test_fk_on_update_action(self):
        """Test FK on_update action configuration."""

        class Parent(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        class Child(Model):
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

        class Comment(Model):
            id: int | None = Field(default=None, db_pk=True)
            post_id: int = 0
            body: str = ""

            class Meta:
                is_table = True

        class Post(Model):
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

        class Reply(Model):
            id: int | None = Field(default=None, db_pk=True)
            message_id: int = 0

            class Meta:
                is_table = True

        class Message(Model):
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

        class Item(Model):
            id: int | None = Field(default=None, db_pk=True)
            container_id: int = 0

            class Meta:
                is_table = True

        class Container(Model):
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

        class Tag(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class PostTag(Model):
            id: int | None = Field(default=None, db_pk=True)
            post_id: int = 0
            tag_id: int = 0

            class Meta:
                is_table = True

        class Article(Model):
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

        class Skill(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class EmployeeSkill(Model):
            id: int | None = Field(default=None, db_pk=True)
            employee_id: int = 0
            skill_id: int = 0

            class Meta:
                is_table = True

        class Employee(Model):
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

        class Writer(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str = ""

            class Meta:
                is_table = True

        class Blog(Model):
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
        """Test that join properly hydrates related models from columnar format."""

        class Creator(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str = ""

            class Meta:
                is_table = True

        class Story(Model):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            creator: Creator | None = None

            class Meta:
                is_table = True

        # Columnar result format (as returned by Rust for all SELECT queries)
        columnar_result = (
            ["id", "title", "creator__id", "creator__email"],
            [[1, "Story 1", 10, "author@example.com"]],
        )

        stub = StubExecuteClient([columnar_result])
        stories = await Story.objects.join("creator").fetch_models(stub)

        assert len(stories) == 1
        assert stories[0].creator is not None
        assert stories[0].creator.email == "author@example.com"

    @pytest.mark.asyncio
    async def test_join_creates_separate_instances(self):
        """Test that join creates separate related model instances per row.

        Unlike deduplication, each row gets its own related model instance,
        matching Tortoise/Django behavior. This is simpler and uses less
        memory overall due to columnar format.
        """

        class Publisher(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Book(Model):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            publisher: Publisher | None = None

            class Meta:
                is_table = True

        # 5 books from 2 publishers - each row has full publisher data
        columnar_result = (
            ["id", "title", "publisher__id", "publisher__name"],
            [
                [1, "Book 1", 1, "Penguin"],
                [2, "Book 2", 1, "Penguin"],
                [3, "Book 3", 1, "Penguin"],
                [4, "Book 4", 2, "HarperCollins"],
                [5, "Book 5", 2, "HarperCollins"],
            ],
        )

        stub = StubExecuteClient([columnar_result])
        books = await Book.objects.join("publisher").fetch_models(stub)

        assert len(books) == 5

        # Publisher instances are cached by PK - same publisher == same object
        assert books[0].publisher.name == "Penguin"
        assert books[3].publisher.name == "HarperCollins"

        # Same publisher ID -> same instance (PK-based caching)
        assert books[0].publisher is books[1].publisher  # Both have publisher_id=1
        assert books[0].publisher is books[2].publisher  # Also publisher_id=1
        assert books[3].publisher is books[4].publisher  # Both have publisher_id=2

        # Different publishers -> different instances
        assert books[0].publisher is not books[3].publisher

        # Count unique instances - should be 2 (one per unique publisher PK)
        unique_publishers = set(id(book.publisher) for book in books)
        assert len(unique_publishers) == 2


class TestPrefetchRelations:
    """Test prefetch operations with relations."""

    def test_prefetch_generates_prefetch_spec(self):
        """Test that prefetch() stores prefetch paths in QuerySet."""

        class Review(Model):
            id: int | None = Field(default=None, db_pk=True)
            product_id: int = 0
            rating: int = 0

            class Meta:
                is_table = True

        class Product(Model):
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

        class Note(Model):
            id: int | None = Field(default=None, db_pk=True)
            task_id: int = 0
            text: str = ""

            class Meta:
                is_table = True

        class Task(Model):
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

        class TreeNode(Model):
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

        class Category(Model):
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

        class SimpleModel(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        with pytest.raises(ValueError):
            SimpleModel.objects.join()

    def test_prefetch_requires_path(self):
        """Test that prefetch() requires at least one path."""

        class SimpleModel(Model):
            id: int | None = Field(default=None, db_pk=True)

            class Meta:
                is_table = True

        with pytest.raises(ValueError):
            SimpleModel.objects.prefetch()


class TestDbFkParameter:
    """Test db_fk parameter for explicit FK target specification."""

    def test_db_fk_with_model_type_targets_pk_by_default(self):
        """Test FK to model type targets PK by default."""

        class Account(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Profile(Model):
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

        class Tenant(Model):
            id: int | None = Field(default=None, db_pk=True)
            uuid: str = Field(db_unique=True)

            class Meta:
                is_table = True

        class Resource(Model):
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

        class Organization(Model):
            uuid: str = Field(db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Member(Model):
            id: int | None = Field(default=None, db_pk=True)
            org: Organization = Field()  # No db_fk - should auto-detect uuid as PK

            class Meta:
                is_table = True

        registered_tables()

        meta = Member._db_meta.field_metadata["org"]
        assert meta.foreign_key is not None
        assert meta.foreign_key.target_field == "uuid"
        assert meta.foreign_key.column_name == "org_uuid"  # {field_name}_{pk_field}


class TestM2MPrefetchExecution:
    """Test M2M prefetch execution (not just metadata)."""

    @pytest.mark.asyncio
    async def test_m2m_prefetch_populates_relation(self):
        """Test that prefetch properly populates M2M relation."""

        class Tag(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Post(Model):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            tags: list[Tag] = Field(db_m2m=True, db_through="PostTag")

            class Meta:
                is_table = True

        class PostTag(Model):
            """Through model with proper FK fields (optional for validation)."""

            id: int | None = Field(default=None, db_pk=True)
            # FK fields optional so Pydantic accepts raw rows with just *_id
            post: Post | None = Field(default=None, db_on_delete="CASCADE")
            tag: Tag | None = Field(default=None, db_on_delete="CASCADE")

            class Meta:
                is_table = True

        registered_tables()

        # Responses: 1) main posts, 2) through table links, 3) target tags
        post_rows = [[{"id": 1, "title": "Post 1"}]]
        # Through table returns FK column values (post_id, tag_id)
        link_rows = [
            [
                {"id": 1, "post_id": 1, "tag_id": 10},
                {"id": 2, "post_id": 1, "tag_id": 20},
            ]
        ]
        tag_rows = [
            [
                {"id": 10, "name": "python"},
                {"id": 20, "name": "rust"},
            ]
        ]

        stub = StubExecuteClient(post_rows + link_rows + tag_rows)
        posts = await Post.objects.prefetch("tags").fetch_models(stub)

        assert len(posts) == 1
        assert len(posts[0].tags) == 2
        tag_names = {t.name for t in posts[0].tags}
        assert tag_names == {"python", "rust"}


class TestDedupHydration:
    """Test dedup format hydration for JOIN queries."""

    @pytest.mark.asyncio
    async def test_dedup_hydration_reuses_instances(self):
        """Test that dedup format correctly hydrates and reuses related instances."""

        class Author(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Article(Model):
            id: int | None = Field(default=None, db_pk=True)
            title: str = ""
            author_id: int | None = None
            author: Author | None = None

            class Meta:
                is_table = True

        registered_tables()

        class DedupStubClient:
            """Stub that simulates execute_batched_dedup response."""

            def __init__(self, dedup_result: dict):
                self._result = dedup_result
                self.calls: list = []

            async def execute(self, ir: dict) -> bytes:
                self.calls.append(ir)
                return msgpack.packb([])

            async def execute_batched_dedup(self, ir: dict) -> dict:
                self.calls.append(ir)
                return self._result

        # Dedup format: main rows + deduplicated relations
        dedup_result = {
            "main": [
                {"id": 1, "title": "Article 1", "author_id": 100},
                {"id": 2, "title": "Article 2", "author_id": 100},
                {"id": 3, "title": "Article 3", "author_id": 200},
            ],
            "relations": {
                "author": {
                    100: {"id": 100, "name": "Alice"},
                    200: {"id": 200, "name": "Bob"},
                }
            },
        }

        stub = DedupStubClient(dedup_result)
        articles = await Article.objects.join("author").fetch_models(stub)

        assert len(articles) == 3

        # Check hydration
        assert articles[0].author is not None
        assert articles[0].author.name == "Alice"
        assert articles[2].author.name == "Bob"

        # Check instance reuse - same author_id should be same instance
        assert articles[0].author is articles[1].author


class TestNestedJoinHydration:
    """Test nested JOIN hydration (multi-level)."""

    @pytest.mark.asyncio
    async def test_nested_join_hydrates_correctly(self):
        """Test that nested JOINs hydrate related models at multiple levels."""

        class Country(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Profile(Model):
            id: int | None = Field(default=None, db_pk=True)
            bio: str = ""
            country: Country | None = None

            class Meta:
                is_table = True

        class User(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""
            profile: Profile | None = None

            class Meta:
                is_table = True

        registered_tables()

        # Columnar format with nested joins: user -> profile -> country
        columnar_result = (
            [
                "id",
                "name",
                "profile__id",
                "profile__bio",
                "profile__country__id",
                "profile__country__name",
            ],
            [[1, "Alice", 10, "Developer", 100, "USA"]],
        )

        stub = StubExecuteClient([columnar_result])

        # Build query with nested joins
        query = User.objects.join("profile").join("profile__country")
        users = await query.fetch_models(stub)

        assert len(users) == 1
        assert users[0].name == "Alice"
        assert users[0].profile is not None
        assert users[0].profile.bio == "Developer"
        assert users[0].profile.country is not None
        assert users[0].profile.country.name == "USA"

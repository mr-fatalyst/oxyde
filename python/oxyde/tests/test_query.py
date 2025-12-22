from __future__ import annotations

from datetime import datetime
from types import SimpleNamespace
from typing import Any

import msgpack
import pytest

from oxyde import Field, OxydeModel
from oxyde.db import atomic
from oxyde.db.transaction import get_active_transaction
from oxyde.exceptions import (
    FieldError,
    FieldLookupError,
    FieldLookupValueError,
    ManagerError,
    MultipleObjectsReturned,
    NotFoundError,
)
from oxyde.models.registry import clear_registry, registered_tables
from oxyde.queries import F


class StubExecuteClient:
    def __init__(self, payloads: list[Any]):
        self.payloads = list(payloads)
        self.calls: list[dict[str, Any]] = []

    async def execute(self, ir: dict[str, Any]) -> bytes:
        self.calls.append(ir)
        if not self.payloads:
            raise RuntimeError("stub payloads exhausted")
        payload = self.payloads.pop(0)
        if isinstance(payload, bytes):
            return payload
        return msgpack.packb(payload)


@pytest.fixture
def atomic_stub_env(monkeypatch: pytest.MonkeyPatch):
    call_log: list[tuple[str, Any]] = []

    class DummyTransaction:
        _next_id = 1

        def __init__(self, database: Any, timeout: float | None = None):
            self.database = database
            self.timeout = timeout
            self.id = DummyTransaction._next_id
            DummyTransaction._next_id += 1

        async def __aenter__(self) -> DummyTransaction:
            call_log.append(("enter", self.database.name))
            return self

        async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
            call_log.append(("exit", exc_type is not None))

        async def execute(self, ir: dict[str, Any]) -> bytes:
            call_log.append(("execute", ir["op"]))
            return msgpack.packb([])

    async def dummy_get_connection(
        name: str = "default",
        ensure_connected: bool = True,
    ) -> Any:
        return SimpleNamespace(name=name)

    async def dummy_create_savepoint(tx_id: int, name: str) -> None:
        call_log.append(("savepoint", name))

    async def dummy_release_savepoint(tx_id: int, name: str) -> None:
        call_log.append(("release", name))

    async def dummy_rollback_to_savepoint(tx_id: int, name: str) -> None:
        call_log.append(("rollback_savepoint", name))

    import importlib

    tx_module = importlib.import_module("oxyde.db.transaction")
    monkeypatch.setattr(tx_module, "AsyncTransaction", DummyTransaction)
    monkeypatch.setattr(tx_module, "_create_savepoint", dummy_create_savepoint)
    monkeypatch.setattr(tx_module, "_release_savepoint", dummy_release_savepoint)
    monkeypatch.setattr(
        tx_module, "_rollback_to_savepoint", dummy_rollback_to_savepoint
    )
    reg_module = importlib.import_module("oxyde.db.registry")
    monkeypatch.setattr(reg_module, "get_connection", dummy_get_connection)

    return call_log


class User(OxydeModel):
    id: int | None = Field(default=None, db_pk=True)
    email: str
    is_active: bool = True

    class Meta:
        is_table = True


def test_queryset_filter_uses_field_helper() -> None:
    query = User.objects.filter(email="ada@example.com")
    ir = query.to_ir()

    assert ir["filter_tree"]["field"] == "email"
    assert ir["filter_tree"]["value"] == "ada@example.com"


def test_instance_attribute_access_still_returns_value() -> None:
    user = User(id=1, email="foo@example.com", is_active=False)
    assert user.email == "foo@example.com"
    assert user.is_active is False


def test_meta_is_table_registers_only_tables() -> None:
    clear_registry()

    class Author(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)

        class Meta:
            is_table = True

    class AuthorCreate(Author):
        pass

    class AuthorResponse(Author):
        class Meta:
            is_table = False

    tables = registered_tables()
    author_key = f"{Author.__module__}.{Author.__qualname__}"
    create_key = f"{AuthorCreate.__module__}.{AuthorCreate.__qualname__}"
    response_key = f"{AuthorResponse.__module__}.{AuthorResponse.__qualname__}"

    assert author_key in tables
    assert tables[author_key] is Author
    assert create_key not in tables
    assert response_key not in tables

    assert Author._is_table is True
    assert AuthorCreate._is_table is False
    assert AuthorResponse._is_table is False

    clear_registry()


def test_field_metadata_captures_db_attributes() -> None:
    clear_registry()

    class BaseUser(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        email: str

        class Meta:
            is_table = True

    class Article(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str = Field(max_length=200, db_index=True, db_comment="Article title")
        author: BaseUser = Field(db_on_delete="CASCADE")
        editor: BaseUser | None = None
        slug: str = Field(db_unique=True)

        class Meta:
            is_table = True

    # Ensure metadata is materialised via registry access
    registered_tables()

    user_meta = BaseUser._db_meta
    # db_type is NOT auto-inferred - only set if user explicitly specifies Field(db_type="...")
    # Type inference happens at schema extraction time based on dialect
    assert user_meta.field_metadata["email"].db_type is None
    assert user_meta.field_metadata["email"].python_type == str

    article_meta = Article._db_meta

    id_meta = article_meta.field_metadata["id"]
    assert id_meta.primary_key is True

    title_meta = article_meta.field_metadata["title"]
    # db_type is NOT auto-inferred, but max_length is captured
    assert title_meta.db_type is None
    assert title_meta.max_length == 200
    assert title_meta.index is True
    assert title_meta.comment == "Article title"
    assert title_meta.nullable is False

    author_meta = article_meta.field_metadata["author"]
    assert author_meta.foreign_key is not None
    assert author_meta.foreign_key.column_name == "author_id"
    assert author_meta.foreign_key.on_delete == "CASCADE"
    assert author_meta.nullable is False

    editor_meta = article_meta.field_metadata["editor"]
    assert editor_meta.foreign_key is not None
    assert editor_meta.foreign_key.column_name == "editor_id"
    assert editor_meta.nullable is True

    slug_meta = article_meta.field_metadata["slug"]
    assert slug_meta.unique is True

    clear_registry()


def test_relation_descriptor_registers_metadata() -> None:
    clear_registry()

    class Comment(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        post_id: int = 0

        class Meta:
            is_table = True

    class BlogPost(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        comments: list[Comment] = Field(db_reverse_fk="post_id")

        class Meta:
            is_table = True

    # Need to trigger metadata parsing
    BlogPost.ensure_field_metadata()

    relations = BlogPost._db_meta.relations
    assert "comments" in relations
    info = relations["comments"]
    assert info.kind == "one_to_many"
    assert info.remote_field == "post_id"

    clear_registry()


def test_lookup_generation_for_strings_and_numbers() -> None:
    clear_registry()

    class Product(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        rating: int | None = None

        class Meta:
            is_table = True

    ir = Product.objects.filter(title__icontains="foo").to_ir()
    cond = ir["filter_tree"]
    assert cond["operator"] == "ILIKE"
    assert cond["value"] == "%foo%"

    ir = Product.objects.filter(title__startswith="Bar").to_ir()
    cond = ir["filter_tree"]
    assert cond["operator"] == "LIKE"
    assert cond["value"] == "Bar%"

    ir = Product.objects.filter(rating__gt=10).to_ir()
    cond = ir["filter_tree"]
    assert cond["operator"] == ">"
    assert cond["value"] == 10

    ir = Product.objects.filter(rating__between=(1, 5)).to_ir()
    cond = ir["filter_tree"]
    assert cond["operator"] == "BETWEEN"
    assert cond["value"] == [1, 5]

    ir = Product.objects.filter(rating__isnull=True).to_ir()
    cond = ir["filter_tree"]
    assert cond["operator"] == "IS NULL"

    ir = Product.objects.filter(rating=None).to_ir()
    cond = ir["filter_tree"]
    assert cond["operator"] == "IS NULL"

    # Signature tests removed - filter() accepts **kwargs dynamically

    with pytest.raises(FieldLookupError):
        Product.objects.filter(title__unknown="foo")

    with pytest.raises(FieldLookupValueError):
        Product.objects.filter(title__contains=123)

    with pytest.raises(FieldError):
        Product.objects.filter(missing="foo")

    clear_registry()


@pytest.mark.asyncio
async def test_async_manager_all_modes() -> None:
    clear_registry()

    class Customer(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        email: str

        class Meta:
            is_table = True

    payload = [{"id": 1, "email": "foo@example.com"}]
    stub = StubExecuteClient([payload, payload, payload])

    models = await Customer.objects.all(client=stub)
    assert len(models) == 1
    assert isinstance(models[0], Customer)

    rows = await Customer.objects.all(client=stub, mode="dict")
    assert rows == payload

    raw = await Customer.objects.all(client=stub, mode="msgpack")
    assert raw == msgpack.packb(payload)

    clear_registry()


@pytest.mark.asyncio
async def test_async_manager_get_variants() -> None:
    clear_registry()

    class Article(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str

        class Meta:
            is_table = True

    stub_ok = StubExecuteClient([[{"id": 1, "title": "Hello"}]])
    article = await Article.objects.get(client=stub_ok, title="Hello")
    assert isinstance(article, Article)
    assert article.title == "Hello"

    stub_none = StubExecuteClient([[]])
    with pytest.raises(NotFoundError):
        await Article.objects.get(client=stub_none, title="missing")

    stub_multi = StubExecuteClient([[{"id": 1, "title": "A"}, {"id": 2, "title": "B"}]])
    with pytest.raises(MultipleObjectsReturned):
        await Article.objects.get(client=stub_multi, title__icontains="a")

    stub_optional = StubExecuteClient([[]])
    assert (
        await Article.objects.get_or_none(client=stub_optional, title="missing") is None
    )

    clear_registry()


@pytest.mark.asyncio
async def test_async_manager_first_last_and_count() -> None:
    clear_registry()

    class LogEntry(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        created_at: datetime

        class Meta:
            is_table = True

    first_payload = [{"id": 1, "created_at": datetime.utcnow().isoformat()}]
    last_payload = [{"id": 5, "created_at": datetime.utcnow().isoformat()}]
    count_payload = [{"_count": 2}]

    stub = StubExecuteClient([first_payload, last_payload])

    first = await LogEntry.objects.first(client=stub)
    assert isinstance(first, LogEntry)

    last = await LogEntry.objects.last(client=stub)
    assert isinstance(last, LogEntry)

    count = await LogEntry.objects.count(client=StubExecuteClient([count_payload]))
    assert count == 2

    clear_registry()


@pytest.mark.asyncio
async def test_async_manager_create_update_delete_and_save() -> None:
    clear_registry()

    class Item(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str
        price: int | None = None

        class Meta:
            is_table = True

    create_stub = StubExecuteClient([{
        "columns": ["id", "name", "price"],
        "rows": [[1, "Widget", None]]
    }])
    item = await Item.objects.create(client=create_stub, name="Widget")
    assert isinstance(item, Item)
    assert create_stub.calls[0]["op"] == "insert"
    assert create_stub.calls[0]["values"]["name"] == "Widget"

    item.id = 10
    item.name = "Gadget"
    item.price = 5

    # Test save() - should update all fields (no dirty tracking)
    update_stub = StubExecuteClient([{
        "columns": ["id", "name", "price"],
        "rows": [[10, "Gadget", 5]]
    }])
    await item.save(client=update_stub)
    assert update_stub.calls[0]["op"] == "update"
    assert update_stub.calls[0]["values"]["name"] == "Gadget"
    assert update_stub.calls[0]["values"]["price"] == 5

    update_count_stub = StubExecuteClient([{
        "columns": ["id", "name", "price"],
        "rows": [[1, "a", 100], [2, "b", 100], [3, "c", 100]]
    }])
    rows = await Item.objects.filter(name__icontains="gad").update(
        price=100,
        client=update_count_stub,
    )
    assert len(rows) == 3
    assert update_count_stub.calls[0]["op"] == "update"

    delete_stub = StubExecuteClient([{"affected": 2}])
    deleted = await Item.objects.filter(name__icontains="widget").delete(
        client=delete_stub
    )
    assert deleted == 2
    assert delete_stub.calls[0]["op"] == "delete"

    clear_registry()


def test_queryset_values_distinct_and_slicing() -> None:
    clear_registry()

    class Sample(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        email: str

        class Meta:
            is_table = True

    query = Sample.objects.values("id", "email")
    ir = query.to_ir()
    assert ir["cols"] == ["id", "email"]
    assert query._result_mode == "dict"

    list_query = Sample.objects.values_list("id", flat=True)
    ir_list = list_query.to_ir()
    assert ir_list["cols"] == ["id"]
    assert list_query._values_flat is True

    with pytest.raises(ValueError):
        Sample.objects.values_list("id", "email", flat=True)

    distinct_query = Sample.objects.distinct()
    distinct_ir = distinct_query.to_ir()
    assert distinct_ir.get("distinct") is True

    sliced = Sample.objects.filter()[5:10]
    slice_ir = sliced.to_ir()
    assert slice_ir["offset"] == 5
    assert slice_ir["limit"] == 5

    single = Sample.objects.filter()[3]
    single_ir = single.to_ir()
    assert single_ir["offset"] == 3
    assert single_ir["limit"] == 1

    clear_registry()


@pytest.mark.asyncio
async def test_values_list_execution_returns_expected_shapes() -> None:
    clear_registry()

    class Sample(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        email: str

        class Meta:
            is_table = True

    stub_flat = StubExecuteClient([[{"id": 1}, {"id": 2}]])
    flat_result = await Sample.objects.values_list("id", flat=True).fetch_all(stub_flat)
    assert flat_result == [1, 2]

    stub_dict = StubExecuteClient([[{"id": 1, "email": "a"}, {"id": 2, "email": "b"}]])
    dict_result = await Sample.objects.values("id", "email").fetch_all(stub_dict)
    assert dict_result == [{"id": 1, "email": "a"}, {"id": 2, "email": "b"}]

    stub_tuple = StubExecuteClient([[{"id": 1, "email": "a"}]])
    tuple_result = await Sample.objects.values_list("id", "email").fetch_all(stub_tuple)
    assert tuple_result == [(1, "a")]

    clear_registry()


def test_f_expression_serialization() -> None:
    from oxyde.queries.expressions import _serialize_value_for_ir

    # Test F expression serialization directly
    serialized = _serialize_value_for_ir(F("value") + 1)
    expr = serialized["__expr__"]
    assert expr["type"] == "op"
    assert expr["op"] == "add"
    assert expr["lhs"]["type"] == "column"
    assert expr["rhs"]["type"] == "value"


def test_join_ir_contains_join_spec() -> None:
    clear_registry()

    class Author(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        email: str

        class Meta:
            is_table = True

    class Post(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        author: Author | None = None

        class Meta:
            is_table = True

    ir = Post.objects.join("author").to_ir()
    assert "joins" in ir
    join_spec = ir["joins"][0]
    assert join_spec["path"] == "author"
    columns = {column["field"] for column in join_spec["columns"]}
    assert "email" in columns
    # FK column is in join spec
    assert join_spec["source_column"] == "author_id"

    clear_registry()


def test_year_month_day_lookups() -> None:
    clear_registry()


@pytest.mark.asyncio
async def test_join_hydrates_related_models() -> None:
    clear_registry()

    class Author(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        email: str

        class Meta:
            is_table = True

    class Post(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        author: Author | None = None

        class Meta:
            is_table = True

    # Columnar result format (as returned by Rust for all SELECT queries)
    # Columns include both main model fields and joined fields with prefix
    columnar_result = (
        ["id", "title", "author__id", "author__email"],
        [[1, "First", 10, "ada@example.com"]],
    )
    stub = StubExecuteClient([columnar_result])
    posts = await Post.objects.join("author").fetch_models(stub)
    assert posts[0].author is not None
    assert posts[0].author.email == "ada@example.com"

    clear_registry()


@pytest.mark.asyncio
async def test_prefetch_populates_reverse_relation() -> None:
    clear_registry()

    class Comment(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        post_id: int = 0
        body: str = ""

        class Meta:
            is_table = True

    class Post(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        author_id: int | None = None
        comments: list[Comment] = Field(db_reverse_fk="post_id")

        class Meta:
            is_table = True

    base_rows = [[{"id": 1, "title": "Hello", "author_id": 2}]]
    comment_rows = [[{"id": 5, "post_id": 1, "body": "Nice"}]]
    stub = StubExecuteClient(base_rows + comment_rows)
    posts = await Post.objects.prefetch("comments").fetch_models(stub)
    assert len(posts[0].comments) == 1
    assert posts[0].comments[0].body == "Nice"

    clear_registry()

    class Event(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        created_at: datetime

        class Meta:
            is_table = True

    # Year lookup creates AND of two conditions (>= start, < end)
    filter_tree = Event.objects.filter(created_at__year=2024).to_ir()["filter_tree"]
    assert filter_tree["type"] == "and"
    conditions = filter_tree["conditions"]
    assert conditions[0]["operator"] == ">="
    assert conditions[1]["operator"] == "<"

    # Month lookup also creates AND of two conditions
    filter_tree_month = Event.objects.filter(created_at__month=(2024, 3)).to_ir()[
        "filter_tree"
    ]
    assert filter_tree_month["type"] == "and"
    conditions_month = filter_tree_month["conditions"]
    assert conditions_month[0]["operator"] == ">="
    assert conditions_month[1]["operator"] == "<"

    # Day lookup also creates AND of two conditions
    filter_tree_day = Event.objects.filter(created_at__day=(2024, 3, 15)).to_ir()[
        "filter_tree"
    ]
    assert filter_tree_day["type"] == "and"
    conditions_day = filter_tree_day["conditions"]
    assert conditions_day[0]["operator"] == ">="
    assert conditions_day[1]["operator"] == "<"

    with pytest.raises(FieldLookupValueError):
        Event.objects.filter(created_at__month=3)

    with pytest.raises(FieldLookupValueError):
        Event.objects.filter(created_at__day=(2024, 2, 30))

    clear_registry()


@pytest.mark.asyncio
async def test_async_manager_bulk_create_and_get_or_create() -> None:
    clear_registry()

    class Entry(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        value: str

        class Meta:
            is_table = True

    bulk_stub = StubExecuteClient([{"affected": 2, "inserted_ids": [1, 2]}])
    created = await Entry.objects.bulk_create(
        [{"id": 1, "value": "a"}, Entry(id=2, value="b")],
        client=bulk_stub,
    )
    assert len(created) == 2
    assert bulk_stub.calls[0]["op"] == "insert"
    # bulk_create uses single INSERT with multiple VALUES, so only one call

    get_stub = StubExecuteClient([[{"id": 1, "value": "a"}]])
    obj, created_flag = await Entry.objects.get_or_create(client=get_stub, id=1)
    assert created_flag is False
    assert isinstance(obj, Entry)

    gor_stub = StubExecuteClient([[], {"affected": 1, "inserted_ids": [3]}])
    obj2, created_flag2 = await Entry.objects.get_or_create(
        client=gor_stub,
        defaults={"value": "c"},
        id=3,
    )
    assert created_flag2 is True
    assert isinstance(obj2, Entry)
    assert gor_stub.calls[0]["op"] == "select"
    assert gor_stub.calls[1]["op"] == "insert"

    clear_registry()


@pytest.mark.asyncio
async def test_async_manager_upsert_placeholder() -> None:
    clear_registry()

    class Thing(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)

        class Meta:
            is_table = True

    with pytest.raises(ManagerError):
        await Thing.objects.upsert()

    clear_registry()


@pytest.mark.asyncio
async def test_instance_delete_uses_manager() -> None:
    clear_registry()

    class Sensor(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        label: str

        class Meta:
            is_table = True

    sensor = Sensor(id=9, label="S1")
    delete_stub = StubExecuteClient([{"affected": 1}])
    affected = await sensor.delete(client=delete_stub)
    assert affected == 1
    assert delete_stub.calls[0]["op"] == "delete"

    clear_registry()


@pytest.mark.asyncio
async def test_transaction_atomic_reuses_transaction(
    atomic_stub_env: list[tuple[str, Any]],
) -> None:
    call_log = atomic_stub_env
    clear_registry()

    class Sample(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        value: int

        class Meta:
            is_table = True

    async with atomic() as ctx:
        assert get_active_transaction("default") is ctx.transaction
        await Sample.objects.all()
        async with atomic():
            await Sample.objects.count()
            ctx.set_rollback(True)

    assert call_log.count(("enter", "default")) == 1
    assert call_log.count(("exit", True)) == 1
    assert get_active_transaction("default") is None
    clear_registry()


@pytest.mark.asyncio
async def test_transaction_atomic_nested_exception(
    atomic_stub_env: list[tuple[str, Any]],
) -> None:
    call_log = atomic_stub_env
    clear_registry()

    class Record(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)

        class Meta:
            is_table = True

    with pytest.raises(RuntimeError):
        async with atomic():
            async with atomic():
                raise RuntimeError("boom")

    assert call_log.count(("enter", "default")) == 1
    assert call_log.count(("exit", True)) == 1
    clear_registry()


@pytest.mark.asyncio
async def test_async_manager_unimplemented_create() -> None:
    clear_registry()

    class Dummy(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str

        class Meta:
            is_table = True

    with pytest.raises(ManagerError):
        await Dummy.objects.create()

    clear_registry()


def test_query_select_requires_non_empty_column_list() -> None:
    """Test that select() method requires at least one column."""
    query = User.objects.filter()
    with pytest.raises(ValueError):
        query.select()


@pytest.mark.asyncio
async def test_query_all_executes_models() -> None:
    """Test that query.all() returns model instances."""
    payload = [{"id": 1, "email": "ada@example.com", "is_active": True}]
    stub = StubExecuteClient([payload])
    users = await User.objects.all(client=stub)
    assert len(users) == 1
    assert users[0].email == "ada@example.com"


@pytest.mark.asyncio
async def test_query_all_respects_values_mode() -> None:
    """Test that values_list with flat=True returns list of values."""
    payload = [{"email": "ada@example.com"}]
    stub = StubExecuteClient([payload])
    emails = await User.objects.values_list("email", flat=True).all(client=stub)
    assert emails == ["ada@example.com"]


@pytest.mark.asyncio
async def test_query_all_conflicting_execution_args() -> None:
    """Test that providing both client and using raises error."""
    stub = StubExecuteClient([[]])
    with pytest.raises(ManagerError):
        await User.objects.all(client=stub, using="default")


# ==========================
# FK traversal filter tests
# ==========================


def test_fk_filter_parses_nested_path() -> None:
    """Test that user__age__gte parses to correct field path and lookup."""
    clear_registry()

    class Author(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        age: int = 0

        class Meta:
            is_table = True

    class Post(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        author: Author = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    # Ensure metadata is populated
    Post.ensure_field_metadata()

    # Filter by FK traversal
    query = Post.objects.filter(author__age__gte=18)
    ir = query.to_ir()

    # Should have JOIN
    assert "joins" in ir
    assert len(ir["joins"]) == 1
    join = ir["joins"][0]
    assert join["alias"] == "author"

    # Should have qualified filter (column has alias prefix)
    filter_tree = ir["filter_tree"]
    assert filter_tree["column"] == "author.age"
    assert filter_tree["operator"] == ">="
    assert filter_tree["value"] == 18

    clear_registry()


def test_fk_filter_exact_lookup() -> None:
    """Test FK traversal with exact (default) lookup."""
    clear_registry()

    class Writer(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str

        class Meta:
            is_table = True

    class Article(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        writer: Writer = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    Article.ensure_field_metadata()

    query = Article.objects.filter(writer__name="Alice")
    ir = query.to_ir()

    assert len(ir["joins"]) == 1
    assert ir["filter_tree"]["column"] == "writer.name"
    assert ir["filter_tree"]["operator"] == "="
    assert ir["filter_tree"]["value"] == "Alice"

    clear_registry()


def test_fk_filter_string_lookups() -> None:
    """Test FK traversal with string lookups like icontains."""
    clear_registry()

    class Person(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        email: str

        class Meta:
            is_table = True

    class Comment(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        person: Person = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    Comment.ensure_field_metadata()

    query = Comment.objects.filter(person__email__icontains="@gmail")
    ir = query.to_ir()

    assert len(ir["joins"]) == 1
    assert ir["filter_tree"]["column"] == "person.email"
    assert ir["filter_tree"]["operator"] == "ILIKE"
    assert ir["filter_tree"]["value"] == "%@gmail%"

    clear_registry()


def test_fk_filter_in_q_expression() -> None:
    """Test FK traversal works in Q expressions with AND/OR."""
    clear_registry()

    from oxyde.queries.q import Q

    class Owner(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        verified: bool = False

        class Meta:
            is_table = True

    class Pet(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str
        owner: Owner = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    Pet.ensure_field_metadata()

    # Q expression with FK traversal and OR
    query = Pet.objects.filter(
        Q(name="Fluffy") | Q(owner__verified=True)
    )
    ir = query.to_ir()

    # Should have JOIN from FK traversal
    assert len(ir["joins"]) == 1

    # Filter should be OR
    filter_tree = ir["filter_tree"]
    assert filter_tree["type"] == "or"
    conditions = filter_tree["conditions"]
    assert len(conditions) == 2

    # First child: name = "Fluffy" (no FK traversal, field name)
    assert conditions[0]["field"] == "name"
    assert conditions[0]["value"] == "Fluffy"

    # Second child: owner.verified = True (FK traversal, column has alias)
    assert conditions[1]["column"] == "owner.verified"
    assert conditions[1]["value"] is True

    clear_registry()


def test_fk_filter_exclude() -> None:
    """Test FK traversal in exclude()."""
    clear_registry()

    class Account(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        active: bool = True

        class Meta:
            is_table = True

    class Order(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        account: Account = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    Order.ensure_field_metadata()

    query = Order.objects.exclude(account__active=False)
    ir = query.to_ir()

    assert len(ir["joins"]) == 1

    # Should have NOT wrapper
    filter_tree = ir["filter_tree"]
    assert filter_tree["type"] == "not"
    inner = filter_tree["condition"]
    assert inner["column"] == "account.active"
    assert inner["value"] is False

    clear_registry()


def test_fk_filter_invalid_path_raises() -> None:
    """Test that invalid FK path raises appropriate error."""
    clear_registry()

    class Category(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str

        class Meta:
            is_table = True

    class Product(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        category: Category = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    Product.ensure_field_metadata()

    # nonexistent field in FK chain
    with pytest.raises(FieldError):
        Product.objects.filter(category__nonexistent=True)

    # invalid lookup on FK field
    with pytest.raises(FieldLookupError):
        Product.objects.filter(category__name__badlookup="test")

    clear_registry()


# ================================
# Multi-level FK traversal tests
# ================================


def test_multi_level_fk_traversal() -> None:
    """Test FK traversal through multiple levels: user__profile__country__name."""
    clear_registry()

    class Country(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str

        class Meta:
            is_table = True

    class Profile(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        country: Country = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    class User(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        profile: Profile = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    class Post(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        user: User = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    Post.ensure_field_metadata()

    # 3-level traversal: post -> user -> profile -> country
    query = Post.objects.filter(user__profile__country__name="USA")
    ir = query.to_ir()

    # Should have 3 JOINs
    assert "joins" in ir
    assert len(ir["joins"]) == 3

    # Filter should reference the deepest alias
    filter_tree = ir["filter_tree"]
    assert "country" in filter_tree["column"]
    assert filter_tree["value"] == "USA"

    clear_registry()


def test_q_multiple_fk_paths() -> None:
    """Test Q expression with multiple different FK paths."""
    clear_registry()

    from oxyde.queries.q import Q

    class Author(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        age: int = 0

        class Meta:
            is_table = True

    class Editor(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str

        class Meta:
            is_table = True

    class Article(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        title: str
        author: Author = Field(db_on_delete="CASCADE")
        editor: Editor = Field(db_on_delete="CASCADE")

        class Meta:
            is_table = True

    Article.ensure_field_metadata()

    # Two different FK traversals in one Q expression
    query = Article.objects.filter(
        Q(author__age__gte=18) & Q(editor__name__icontains="smith")
    )
    ir = query.to_ir()

    # Should have 2 JOINs (author and editor)
    assert len(ir["joins"]) == 2
    join_paths = {j["path"] for j in ir["joins"]}
    assert join_paths == {"author", "editor"}

    # Filter should be AND with two conditions
    filter_tree = ir["filter_tree"]
    assert filter_tree["type"] == "and"
    assert len(filter_tree["conditions"]) == 2

    clear_registry()


def test_fk_traversal_with_isnull() -> None:
    """Test FK traversal with isnull lookup."""
    clear_registry()

    class Manager(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str

        class Meta:
            is_table = True

    class Team(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        manager: Manager | None = None

        class Meta:
            is_table = True

    Team.ensure_field_metadata()

    query = Team.objects.filter(manager__name__isnull=True)
    ir = query.to_ir()

    assert len(ir["joins"]) == 1
    assert ir["filter_tree"]["operator"] == "IS NULL"

    clear_registry()

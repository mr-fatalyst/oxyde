"""Tests for QuerySet and Query immutability (_clone behavior)."""

from __future__ import annotations

import pytest

from oxyde import Field, OxydeModel
from oxyde.models.registry import clear_registry


@pytest.fixture(autouse=True)
def cleanup_registry():
    """Clean up registry before and after each test."""
    clear_registry()
    yield
    clear_registry()


class TestModel(OxydeModel):
    """Test model for immutability tests."""

    id: int | None = Field(default=None, db_pk=True)
    name: str
    email: str | None = None
    age: int = 0
    is_active: bool = True

    class Meta:
        is_table = True


class TestQueryImmutability:
    """Test that Query operations return new instances."""

    def test_filter_returns_new_instance(self):
        """Test that filter() returns a new Query instance."""
        base = TestModel.objects.filter()
        filtered = base.filter(name="test")

        assert base is not filtered
        assert "filter_tree" not in base.to_ir()

    def test_limit_returns_new_instance(self):
        """Test that limit() returns a new Query instance."""
        base = TestModel.objects.filter()
        limited = base.limit(10)

        assert base is not limited
        assert base.to_ir().get("limit") is None
        assert limited.to_ir()["limit"] == 10

    def test_offset_returns_new_instance(self):
        """Test that offset() returns a new Query instance."""
        base = TestModel.objects.filter()
        offset = base.offset(5)

        assert base is not offset
        assert base.to_ir().get("offset") is None
        assert offset.to_ir()["offset"] == 5

    def test_order_by_returns_new_instance(self):
        """Test that order_by() returns a new Query instance."""
        base = TestModel.objects.filter()
        ordered = base.order_by("name")

        assert base is not ordered
        assert base.to_ir().get("order_by") is None
        assert ordered.to_ir()["order_by"] is not None

    def test_distinct_returns_new_instance(self):
        """Test that distinct() returns a new Query instance."""
        base = TestModel.objects.filter()
        distinct = base.distinct()

        assert base is not distinct
        assert base.to_ir().get("distinct") is None
        assert distinct.to_ir()["distinct"] is True

    def test_select_returns_new_instance(self):
        """Test that select() returns a new Query instance."""
        base = TestModel.objects.filter()
        selected = base.select("id", "name")

        assert base is not selected

    def test_values_returns_new_instance(self):
        """Test that values() returns a new Query instance."""
        base = TestModel.objects.filter()
        values = base.values("id", "name")

        assert base is not values
        assert values.to_ir()["cols"] == ["id", "name"]

    def test_values_list_returns_new_instance(self):
        """Test that values_list() returns a new Query instance."""
        base = TestModel.objects.filter()
        values_list = base.values_list("id")

        assert base is not values_list

    def test_join_returns_new_instance(self):
        """Test that join() returns a new Query instance when relation exists."""

        # Create models with FK relation - join works from FK side (child -> parent)
        class ParentModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class ChildModel(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            parent: ParentModel = Field(db_on_delete="CASCADE")

            class Meta:
                is_table = True

        # Join works from FK side (ChildModel.parent -> ParentModel)
        base = ChildModel.objects.filter()
        joined = base.join("parent")

        assert base is not joined

    def test_prefetch_returns_new_instance(self):
        """Test that prefetch() returns a new Query instance."""
        base = TestModel.objects.filter()
        prefetched = base.prefetch("items")

        assert base is not prefetched


class TestChainedImmutability:
    """Test immutability with chained operations."""

    def test_chain_preserves_all_original_queries(self):
        """Test that chaining preserves all intermediate queries."""
        q1 = TestModel.objects.filter()
        q2 = q1.filter(name="test")
        q3 = q2.limit(10)
        q4 = q3.offset(5)
        q5 = q4.order_by("name")

        # All should be different instances
        assert q1 is not q2
        assert q2 is not q3
        assert q3 is not q4
        assert q4 is not q5

        # Original query should be unmodified
        ir1 = q1.to_ir()
        assert ir1.get("limit") is None
        assert ir1.get("offset") is None
        assert ir1.get("order_by") is None

        # Final query should have all modifications
        ir5 = q5.to_ir()
        assert ir5["limit"] == 10
        assert ir5["offset"] == 5
        assert ir5["order_by"] is not None

    def test_branching_from_same_base(self):
        """Test creating multiple branches from the same base query."""
        base = TestModel.objects.filter(is_active=True)

        branch1 = base.limit(10)
        branch2 = base.limit(20)
        branch3 = base.order_by("name")

        # All branches should be independent
        assert branch1 is not branch2
        assert branch1 is not branch3
        assert branch2 is not branch3

        # Each should have its own modifications
        assert branch1.to_ir()["limit"] == 10
        assert branch2.to_ir()["limit"] == 20
        assert branch3.to_ir().get("limit") is None

    def test_filter_accumulation(self):
        """Test that filters accumulate correctly."""
        q1 = TestModel.objects.filter()
        q2 = q1.filter(name="test")
        q3 = q2.filter(age__gt=18)

        # q1 should have no filter_tree
        ir1 = q1.to_ir()
        assert ir1.get("filter_tree") is None

        # q2 should have single condition
        ir2 = q2.to_ir()
        tree2 = ir2.get("filter_tree")
        assert tree2 is not None
        assert tree2.get("type") == "condition"

        # q3 should have AND of 2 conditions
        ir3 = q3.to_ir()
        tree3 = ir3.get("filter_tree")
        assert tree3 is not None
        assert tree3.get("type") == "and"
        assert len(tree3.get("conditions", [])) == 2


class TestQuerySetImmutability:
    """Test QuerySet immutability."""

    def test_queryset_filter_returns_query(self):
        """Test that QuerySet.filter() returns immutable Query."""
        base = TestModel.objects.filter(name="test")
        chained = base.filter(age__gte=18)

        assert base is not chained

    def test_queryset_join_returns_new_instance(self):
        """Test that QuerySet.join() returns new QuerySet."""

        class Container(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Item(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            container: Container | None = None  # FK by type hint

            class Meta:
                is_table = True

        base = Item.objects.filter()
        joined = base.join("container")

        assert base is not joined

    def test_queryset_prefetch_returns_new_instance(self):
        """Test that QuerySet.prefetch() returns new QuerySet."""
        base = TestModel.objects.filter()
        prefetched = base.prefetch("items")

        assert base is not prefetched

    def test_queryset_clone_copies_paths(self):
        """Test that QuerySet._clone() copies join and prefetch paths."""

        class Author(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            name: str = ""

            class Meta:
                is_table = True

        class Comment(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            post_id: int = 0
            body: str = ""

            class Meta:
                is_table = True

        class Post(OxydeModel):
            id: int | None = Field(default=None, db_pk=True)
            author: Author | None = None  # FK by type hint
            comments: list[Comment] = Field(db_reverse_fk="post_id")

            class Meta:
                is_table = True

        base = Post.objects.filter()
        with_join = base.join("author")
        with_prefetch = with_join.prefetch("comments")

        # Original should not have joins/prefetches
        assert len(base._join_specs) == 0
        assert len(base._prefetch_paths) == 0

        # Intermediate should have join only
        assert len(with_join._join_specs) == 1
        assert len(with_join._prefetch_paths) == 0

        # Final should have both
        assert len(with_prefetch._join_specs) == 1
        assert len(with_prefetch._prefetch_paths) == 1


class TestSliceImmutability:
    """Test slice notation immutability."""

    def test_slice_returns_new_instance(self):
        """Test that slicing returns a new Query instance."""
        base = TestModel.objects.filter()
        sliced = base[5:10]

        assert base is not sliced

    def test_slice_preserves_original(self):
        """Test that slicing preserves the original query."""
        base = TestModel.objects.filter()
        sliced = base[5:10]

        base_ir = base.to_ir()
        sliced_ir = sliced.to_ir()

        assert base_ir.get("offset") is None
        assert base_ir.get("limit") is None
        assert sliced_ir["offset"] == 5
        assert sliced_ir["limit"] == 5

    def test_index_returns_new_instance(self):
        """Test that indexing returns a new Query instance."""
        base = TestModel.objects.filter()
        indexed = base[3]

        assert base is not indexed

        indexed_ir = indexed.to_ir()
        assert indexed_ir["offset"] == 3
        assert indexed_ir["limit"] == 1


class TestCloneDeepCopy:
    """Test that _clone performs deep copy of mutable attributes."""

    def test_filter_tree_is_independent(self):
        """Test that filter tree is independent between queries."""
        q1 = TestModel.objects.filter(name="test")
        q2 = q1.filter(age__gt=18)

        # q2's filter tree should have both conditions, q1 should only have one
        q1_tree = q1.to_ir().get("filter_tree")
        q2_tree = q2.to_ir().get("filter_tree")

        # q1 has single condition
        assert q1_tree is not None
        assert q1_tree.get("type") == "condition"

        # q2 has AND of two conditions
        assert q2_tree is not None
        assert q2_tree.get("type") == "and"
        assert len(q2_tree.get("conditions", [])) == 2

    def test_order_by_list_is_copied(self):
        """Test that order_by list is deep copied."""
        q1 = TestModel.objects.filter().order_by("name")
        q2 = q1.order_by("-age")

        q1_order = q1.to_ir().get("order_by", [])
        q2_order = q2.to_ir().get("order_by", [])

        assert len(q1_order) == 1
        assert len(q2_order) == 2

    def test_selected_fields_is_copied(self):
        """Test that selected fields list is deep copied."""
        q1 = TestModel.objects.filter().select("id", "name")
        q2 = q1.select("id", "name", "email")

        # Both should have their own field selections
        assert q1 is not q2


class TestAnnotationsImmutability:
    """Test annotations dictionary immutability."""

    def test_annotate_returns_new_instance(self):
        """Test that annotate() returns new instance."""
        from oxyde.queries.aggregates import Count

        base = TestModel.objects.filter()
        annotated = base.annotate(total=Count("*"))

        assert base is not annotated

    def test_group_by_returns_new_instance(self):
        """Test that group_by() returns new instance."""
        from oxyde.queries.aggregates import Count

        base = TestModel.objects.filter().annotate(total=Count("*"))
        grouped = base.group_by("name")

        assert base is not grouped

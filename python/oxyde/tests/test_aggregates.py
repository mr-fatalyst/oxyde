"""Tests for aggregate functions: Count, Sum, Avg, Max, Min + group_by."""

from __future__ import annotations

from typing import Any

import msgpack
import pytest

from oxyde import Field, OxydeModel
from oxyde.models.registry import clear_registry
from oxyde.queries.aggregates import (
    Aggregate,
    Avg,
    Coalesce,
    Concat,
    Count,
    Max,
    Min,
    RawSQL,
    Sum,
)


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
        self.calls: list[dict[str, Any]] = []

    async def execute(self, ir: dict[str, Any]) -> bytes:
        self.calls.append(ir)
        if not self.payloads:
            raise RuntimeError("stub payloads exhausted")
        payload = self.payloads.pop(0)
        if isinstance(payload, bytes):
            return payload
        return msgpack.packb(payload)


class Product(OxydeModel):
    """Test model for aggregate tests."""

    id: int | None = Field(default=None, db_pk=True)
    name: str
    category: str = "default"
    price: float = 0.0
    quantity: int = 0
    rating: float | None = None

    class Meta:
        is_table = True


class TestAggregateBase:
    """Test Aggregate base class."""

    def test_aggregate_init(self):
        """Test Aggregate initialization."""
        agg = Aggregate("price")
        assert agg.field == "price"
        assert agg.distinct is False

    def test_aggregate_with_distinct(self):
        """Test Aggregate with distinct=True."""
        agg = Aggregate("category", distinct=True)
        assert agg.distinct is True

    def test_aggregate_to_ir(self):
        """Test Aggregate.to_ir()."""
        agg = Aggregate("price")
        ir = agg.to_ir()

        assert ir["func"] == "aggregate"
        assert ir["field"] == "price"

    def test_aggregate_to_ir_with_distinct(self):
        """Test Aggregate.to_ir() with distinct."""
        agg = Aggregate("price", distinct=True)
        ir = agg.to_ir()

        assert ir["distinct"] is True


class TestCountAggregate:
    """Test Count aggregate function."""

    def test_count_default_star(self):
        """Test Count() defaults to '*'."""
        count = Count()
        assert count.field == "*"

    def test_count_with_field(self):
        """Test Count(field)."""
        count = Count("id")
        assert count.field == "id"

    def test_count_to_ir(self):
        """Test Count.to_ir()."""
        count = Count("id")
        ir = count.to_ir()

        assert ir["func"] == "count"
        assert ir["field"] == "id"

    def test_count_distinct(self):
        """Test Count with distinct."""
        count = Count("category", distinct=True)
        ir = count.to_ir()

        assert ir["func"] == "count"
        assert ir["distinct"] is True


class TestSumAggregate:
    """Test Sum aggregate function."""

    def test_sum_to_ir(self):
        """Test Sum.to_ir()."""
        s = Sum("price")
        ir = s.to_ir()

        assert ir["func"] == "sum"
        assert ir["field"] == "price"


class TestAvgAggregate:
    """Test Avg aggregate function."""

    def test_avg_to_ir(self):
        """Test Avg.to_ir()."""
        avg = Avg("rating")
        ir = avg.to_ir()

        assert ir["func"] == "avg"
        assert ir["field"] == "rating"


class TestMaxAggregate:
    """Test Max aggregate function."""

    def test_max_to_ir(self):
        """Test Max.to_ir()."""
        m = Max("price")
        ir = m.to_ir()

        assert ir["func"] == "max"
        assert ir["field"] == "price"


class TestMinAggregate:
    """Test Min aggregate function."""

    def test_min_to_ir(self):
        """Test Min.to_ir()."""
        m = Min("price")
        ir = m.to_ir()

        assert ir["func"] == "min"
        assert ir["field"] == "price"


class TestConcatAggregate:
    """Test Concat function."""

    def test_concat_to_ir(self):
        """Test Concat.to_ir()."""
        c = Concat("first_name", "last_name")
        ir = c.to_ir()

        assert ir["func"] == "concat"
        assert ir["fields"] == ["first_name", "last_name"]

    def test_concat_with_separator(self):
        """Test Concat with separator."""
        c = Concat("first_name", "last_name", separator=" ")
        ir = c.to_ir()

        assert ir["separator"] == " "


class TestCoalesceAggregate:
    """Test Coalesce function."""

    def test_coalesce_to_ir(self):
        """Test Coalesce.to_ir()."""
        c = Coalesce("nickname", "name", "email")
        ir = c.to_ir()

        assert ir["func"] == "coalesce"
        assert ir["fields"] == ["nickname", "name", "email"]


class TestRawSQL:
    """Test RawSQL expression."""

    def test_raw_sql_to_ir(self):
        """Test RawSQL.to_ir()."""
        raw = RawSQL("CURRENT_TIMESTAMP")
        ir = raw.to_ir()

        assert ir["type"] == "raw"
        assert ir["sql"] == "CURRENT_TIMESTAMP"


class TestAnnotateMethod:
    """Test Query.annotate() method."""

    def test_annotate_with_count(self):
        """Test annotate() with Count."""
        query = Product.objects.filter().annotate(total=Count("*"))
        ir = query.to_ir()

        assert ir["aggregates"] is not None
        assert len(ir["aggregates"]) == 1
        assert ir["aggregates"][0]["op"] == "count"
        assert ir["aggregates"][0]["alias"] == "total"

    def test_annotate_with_sum(self):
        """Test annotate() with Sum."""
        query = Product.objects.filter().annotate(total_price=Sum("price"))
        ir = query.to_ir()

        assert ir["aggregates"][0]["op"] == "sum"
        assert ir["aggregates"][0]["field"] == "price"

    def test_annotate_with_avg(self):
        """Test annotate() with Avg."""
        query = Product.objects.filter().annotate(avg_rating=Avg("rating"))
        ir = query.to_ir()

        assert ir["aggregates"][0]["op"] == "avg"

    def test_annotate_immutability(self):
        """Test that annotate() returns new instance."""
        base = Product.objects.filter()
        annotated = base.annotate(total=Count("*"))

        assert base is not annotated


class TestGroupByMethod:
    """Test Query.group_by() method."""

    def test_group_by_single_field(self):
        """Test group_by() with single field."""
        query = Product.objects.filter().annotate(count=Count("*")).group_by("category")
        ir = query.to_ir()

        assert ir["group_by"] is not None
        assert "category" in ir["group_by"]

    def test_group_by_multiple_fields(self):
        """Test group_by() with multiple fields."""
        query = Product.objects.filter().annotate(count=Count("*")).group_by("category", "name")
        ir = query.to_ir()

        assert len(ir["group_by"]) == 2

    def test_group_by_immutability(self):
        """Test that group_by() returns new instance."""
        base = Product.objects.filter().annotate(count=Count("*"))
        grouped = base.group_by("category")

        assert base is not grouped


class TestHavingMethod:
    """Test Query.having() method."""

    def test_having_with_condition(self):
        """Test having() with condition via kwargs."""
        query = (
            Product.objects.filter()
            .annotate(count=Count("*"))
            .group_by("category")
            .having(id__gt=0)  # Use kwargs for having clause
        )
        ir = query.to_ir()

        assert ir["having"] is not None


class TestManagerAggregates:
    """Test manager aggregate methods."""

    @pytest.mark.asyncio
    async def test_manager_count(self):
        """Test manager.count() method."""
        # count() expects aggregate result with _count field
        stub = StubExecuteClient([[{"_count": 3}]])

        count = await Product.objects.count(client=stub)

        assert count == 3

    @pytest.mark.asyncio
    async def test_manager_sum(self):
        """Test manager.sum() method."""
        stub = StubExecuteClient([[{"__sum__": 150.0}]])

        total = await Product.objects.sum("price", client=stub)

        # Result depends on implementation
        assert stub.calls[0]["aggregates"][0]["op"] == "sum"

    @pytest.mark.asyncio
    async def test_manager_avg(self):
        """Test manager.avg() method."""
        stub = StubExecuteClient([[{"__avg__": 4.5}]])

        avg = await Product.objects.avg("rating", client=stub)

        assert stub.calls[0]["aggregates"][0]["op"] == "avg"

    @pytest.mark.asyncio
    async def test_manager_max(self):
        """Test manager.max() method."""
        stub = StubExecuteClient([[{"__max__": 99.99}]])

        max_val = await Product.objects.max("price", client=stub)

        assert stub.calls[0]["aggregates"][0]["op"] == "max"

    @pytest.mark.asyncio
    async def test_manager_min(self):
        """Test manager.min() method."""
        stub = StubExecuteClient([[{"__min__": 0.99}]])

        min_val = await Product.objects.min("price", client=stub)

        assert stub.calls[0]["aggregates"][0]["op"] == "min"


class TestAggregatesWithFilters:
    """Test aggregates combined with filters."""

    def test_filter_then_annotate(self):
        """Test filter().annotate()."""
        query = Product.objects.filter(category="electronics").annotate(
            total=Count("*")
        )
        ir = query.to_ir()

        assert ir.get("filter_tree") is not None
        assert ir["aggregates"] is not None

    def test_annotate_then_filter(self):
        """Test annotate().filter() - filter applied to base query."""
        query = (
            Product.objects.filter()
            .annotate(total=Count("*"))
            .filter(category="electronics")
        )
        ir = query.to_ir()

        assert ir["aggregates"] is not None


class TestGroupByWithAggregates:
    """Test group_by combined with various aggregates."""

    def test_group_by_with_count(self):
        """Test group_by with Count."""
        query = Product.objects.filter().annotate(count=Count("*")).group_by("category")
        ir = query.to_ir()

        assert ir["aggregates"][0]["op"] == "count"
        assert ir["group_by"] is not None

    def test_group_by_with_sum(self):
        """Test group_by with Sum."""
        query = Product.objects.filter().annotate(total=Sum("price")).group_by("category")
        ir = query.to_ir()

        assert ir["aggregates"][0]["op"] == "sum"
        assert ir["group_by"] is not None

    def test_group_by_with_avg(self):
        """Test group_by with Avg."""
        query = Product.objects.filter().annotate(avg_price=Avg("price")).group_by("category")
        ir = query.to_ir()

        assert ir["aggregates"][0]["op"] == "avg"


class TestMultipleAggregates:
    """Test multiple aggregates in a single query (bug fix verification)."""

    def test_multiple_aggregates_in_annotate(self):
        """Test that multiple aggregates are all included in IR."""
        query = Product.objects.filter().annotate(
            total=Count("*"),
            avg_price=Avg("price"),
            max_price=Max("price"),
        )
        ir = query.to_ir()

        assert ir["aggregates"] is not None
        assert len(ir["aggregates"]) == 3

        # Check all aggregates are present
        ops = {agg["alias"]: agg["op"] for agg in ir["aggregates"]}
        assert ops["total"] == "count"
        assert ops["avg_price"] == "avg"
        assert ops["max_price"] == "max"

    def test_multiple_aggregates_with_group_by(self):
        """Test multiple aggregates with GROUP BY."""
        query = (
            Product.objects.filter()
            .annotate(
                count=Count("*"),
                total_price=Sum("price"),
                min_price=Min("price"),
            )
            .group_by("category")
        )
        ir = query.to_ir()

        assert len(ir["aggregates"]) == 3
        assert ir["group_by"] is not None

    def test_chained_annotate_calls(self):
        """Test chaining multiple annotate() calls."""
        query = (
            Product.objects.filter().annotate(total=Count("*")).annotate(avg_price=Avg("price"))
        )
        ir = query.to_ir()

        assert len(ir["aggregates"]) == 2

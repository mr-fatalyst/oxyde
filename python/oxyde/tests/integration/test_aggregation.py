"""Integration tests for aggregation functions and group_by."""
from __future__ import annotations

from datetime import date
from decimal import Decimal

import pytest

from oxyde import Avg, Count, Max, Min, Sum

from .conftest import AllTypes, Author, Post


class TestAggregateShortcuts:
    @pytest.mark.asyncio
    async def test_count(self, db):
        count = await Post.objects.filter(published=True).count(using=db.name)
        assert count == 4

    @pytest.mark.asyncio
    async def test_count_with_join(self, db):
        """count() must work when join() is applied (no dedup panic)."""
        count = await Post.objects.join("author").count(using=db.name)
        assert count == 6

    @pytest.mark.asyncio
    async def test_count_with_join_and_filter(self, db):
        """count() with join + filter on joined field."""
        count = await Post.objects.join("author").filter(author__name="Alice").count(using=db.name)
        assert count == 2

    @pytest.mark.asyncio
    async def test_exists_with_join(self, db):
        """exists() must work when join() is applied (no dedup panic)."""
        exists = await Post.objects.join("author").exists(using=db.name)
        assert exists is True

    @pytest.mark.asyncio
    async def test_sum(self, db):
        total = await Post.objects.filter(published=True).sum("views", using=db.name)
        assert total == 435  # 120 + 35 + 80 + 200

    @pytest.mark.asyncio
    async def test_avg(self, db):
        average = await Post.objects.filter(published=True).avg("views", using=db.name)
        assert average == pytest.approx(108.75)

    @pytest.mark.asyncio
    async def test_max(self, db):
        maximum = await Post.objects.max("views", using=db.name)
        assert maximum == 200

    @pytest.mark.asyncio
    async def test_min(self, db):
        minimum = await Post.objects.filter(published=True).min("views", using=db.name)
        assert minimum == 35


class TestAnnotateGroupBy:
    @pytest.mark.asyncio
    async def test_count_group_by(self, db):
        """Count posts per author using values() + group_by."""
        rows = await (
            Post.objects.annotate(n=Count("id"))
            .group_by("author_id")
            .order_by("author_id")
            .values("author_id", "n")
            .all(using=db.name)
        )
        assert len(rows) == 3
        counts = [r["n"] for r in rows]
        assert counts == [2, 2, 2]  # Alice=2, Bob=2, Charlie=2

    @pytest.mark.asyncio
    async def test_group_by_without_values_raises(self, db):
        """group_by().all() without values() raises TypeError."""
        with pytest.raises(TypeError, match="group_by.*values"):
            await (
                Post.objects.annotate(n=Count("id"))
                .group_by("author_id")
                .all(using=db.name)
            )

    @pytest.mark.asyncio
    async def test_having(self, db):
        """Group by author_id, keep only those with sum views > 100."""
        rows = await (
            Post.objects.annotate(total=Sum("views"))
            .group_by("author_id")
            .having(total__gt=100)
            .values("author_id", "total")
            .all(using=db.name)
        )
        # Alice: 120+35=155, Bob: 80+0=80, Charlie: 200+0=200
        assert len(rows) == 2  # Alice and Charlie


async def _seed_typed_groups(db):
    # SQLite stores Decimal as TEXT, so HAVING compares it as text there (as
    # it always has); the decimals share their number of integer digits.
    for str_val, decimal_val, date_val in [
        ("a", Decimal("1.25"), date(2024, 1, 10)),
        ("a", Decimal("2.00"), date(2024, 2, 10)),
        ("b", Decimal("3.75"), date(2024, 3, 10)),
        ("b", Decimal("1.50"), date(2024, 1, 5)),
    ]:
        await AllTypes.objects.create(
            str_val=str_val, decimal_val=decimal_val, date_val=date_val, using=db.name
        )


class TestHavingAggregateTypes:
    """HAVING values bind by the aggregate's result type, so string-encoded
    values (Decimal, date) compare as that type."""

    @pytest.mark.asyncio
    @pytest.mark.parametrize("alias", ["top", "decimal_val"])
    async def test_max_decimal(self, db, alias):
        """The alias may also be the aggregated field's name."""
        await _seed_typed_groups(db)
        rows = await (
            AllTypes.objects.annotate(**{alias: Max("decimal_val")})
            .group_by("str_val")
            .having(**{f"{alias}__gt": Decimal("2.5")})
            .values("str_val")
            .all(using=db.name)
        )
        assert [r["str_val"] for r in rows] == ["b"]

    @pytest.mark.asyncio
    async def test_max_date(self, db):
        await _seed_typed_groups(db)
        rows = await (
            AllTypes.objects.annotate(last=Max("date_val"))
            .group_by("str_val")
            .having(last__gt=date(2024, 3, 1))
            .values("str_val")
            .all(using=db.name)
        )
        assert [r["str_val"] for r in rows] == ["b"]

"""Integration tests for aggregation functions and group_by."""
from __future__ import annotations

import pytest

from oxyde import Avg, Count, Max, Min, Sum

from .conftest import Author, Post


class TestAggregateShortcuts:
    @pytest.mark.asyncio
    async def test_count(self, db):
        count = await Post.objects.filter(published=True).count(client=db)
        assert count == 4

    @pytest.mark.asyncio
    async def test_sum(self, db):
        total = await Post.objects.filter(published=True).sum("views", client=db)
        assert total == 435  # 120 + 35 + 80 + 200

    @pytest.mark.asyncio
    async def test_avg(self, db):
        average = await Post.objects.filter(published=True).avg("views", client=db)
        assert average == pytest.approx(108.75)

    @pytest.mark.asyncio
    async def test_max(self, db):
        maximum = await Post.objects.max("views", client=db)
        assert maximum == 200

    @pytest.mark.asyncio
    async def test_min(self, db):
        minimum = await Post.objects.filter(published=True).min("views", client=db)
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
            .all(client=db)
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
                .all(client=db)
            )

    @pytest.mark.asyncio
    async def test_having(self, db):
        """Group by author_id, keep only those with sum views > 100."""
        rows = await (
            Post.objects.annotate(total=Sum("views"))
            .group_by("author_id")
            .having(total__gt=100)
            .values("author_id", "total")
            .all(client=db)
        )
        # Alice: 120+35=155, Bob: 80+0=80, Charlie: 200+0=200
        assert len(rows) == 2  # Alice and Charlie

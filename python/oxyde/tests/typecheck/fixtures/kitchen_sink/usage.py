"""Kitchen-sink usage: exercise every public Manager/Query method."""

from __future__ import annotations

from typing import Any

from models import Author, Post, Tag


# --- Manager-level query builders ---


async def mgr_filter() -> list[Post]:
    return await Post.objects.filter(title__icontains="hi", views__gte=10).all()


async def mgr_exclude() -> list[Post]:
    return await Post.objects.exclude(published=False).all()


async def mgr_values() -> list[Post]:
    return await Post.objects.values("id", "title").all()


async def mgr_values_list() -> list[Post]:
    return await Post.objects.values_list("id", flat=True).all()


async def mgr_distinct() -> list[Post]:
    return await Post.objects.distinct().all()


async def mgr_join() -> list[Post]:
    return await Post.objects.join("author").all()


async def mgr_prefetch() -> list[Post]:
    return await Post.objects.prefetch("tags").all()


async def mgr_for_update() -> list[Post]:
    return await Post.objects.for_update().all()


async def mgr_for_share() -> list[Post]:
    return await Post.objects.for_share().all()


# --- Manager-level terminals ---


async def mgr_get() -> Post:
    return await Post.objects.get(id=1)


async def mgr_get_or_none() -> Post | None:
    return await Post.objects.get_or_none(id=1)


async def mgr_get_or_create() -> tuple[Post, bool]:
    return await Post.objects.get_or_create(title="X", defaults={"views": 0})


async def mgr_update_or_create() -> tuple[Post, bool]:
    return await Post.objects.update_or_create(title="X", defaults={"views": 1})


async def mgr_all() -> list[Post]:
    return await Post.objects.all()


async def mgr_first_last() -> tuple[Post | None, Post | None]:
    return await Post.objects.first(), await Post.objects.last()


async def mgr_count() -> int:
    return await Post.objects.count()


async def mgr_aggregates() -> tuple[Any, Any, Any, Any]:
    return (
        await Post.objects.sum("views"),
        await Post.objects.avg("views"),
        await Post.objects.min("views"),
        await Post.objects.max("views"),
    )


async def mgr_create() -> Post:
    return await Post.objects.create(title="Hi", body="")


async def mgr_bulk_create(items: list[Post]) -> list[Post]:
    return await Post.objects.bulk_create(items, batch_size=100)


async def mgr_bulk_update(items: list[Post]) -> int:
    return await Post.objects.bulk_update(items, ["title"])


# --- Query-level methods (reached via .filter() or .query()) ---


async def q_order_by() -> list[Post]:
    return await Post.objects.filter().order_by("-views", "title").all()


async def q_limit_offset() -> list[Post]:
    return await Post.objects.filter().limit(10).offset(5).all()


async def q_select() -> list[Post]:
    return await Post.objects.filter().select("id", "title").all()


async def q_group_by_having_annotate() -> list[Post]:
    return (
        await Post.objects.filter()
        .group_by("published")
        .annotate(total=1)
        .having()
        .all()
    )


async def q_update_delete() -> tuple[int, int]:
    return (
        await Post.objects.filter(id=1).update(title="X"),
        await Post.objects.filter(id=1).delete(),
    )


async def q_increment() -> int:
    return await Post.objects.filter(id=1).increment("views", by=1)


async def q_query_builder() -> list[Post]:
    return await Post.objects.query().filter().all()


# --- Relations: exercise FK / reverse FK / M2M returning correct model types ---


async def rel_fk() -> list[Author]:
    return await Author.objects.filter(active=True).all()


async def rel_tags() -> list[Tag]:
    return await Tag.objects.filter(name__icontains="py").all()


# --- Various field types exercised in filter lookups ---


async def field_types_filters() -> list[Post]:
    return await (
        Post.objects.filter(
            views__gte=0,
            score__lt=1.0,
            published=True,
            created_at__year=2026,
            slug_id__isnull=False,
            title__startswith="Hi",
        )
        .all()
    )

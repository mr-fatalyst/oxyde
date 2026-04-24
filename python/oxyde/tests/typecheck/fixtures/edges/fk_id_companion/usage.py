from __future__ import annotations

from module import Article


async def filter_by_companion_id() -> list[Article]:
    return await Article.objects.filter(tag_id=1).all()


async def filter_by_companion_id_in() -> list[Article]:
    return await Article.objects.filter(tag_id__in=[1, 2, 3]).all()


async def filter_companion_isnull() -> list[Article]:
    return await Article.objects.filter(tag_id__isnull=True).all()

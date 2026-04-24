from __future__ import annotations

from module import Post


async def filter_by_fk_scalar() -> list[Post]:
    return await Post.objects.filter(author__name__icontains="alice").all()


async def filter_by_fk_isnull() -> list[Post]:
    return await Post.objects.filter(author__isnull=True).all()


async def filter_by_title_and_fk() -> list[Post]:
    return await Post.objects.filter(
        title__startswith="Hello",
        author__name="Bob",
    ).all()

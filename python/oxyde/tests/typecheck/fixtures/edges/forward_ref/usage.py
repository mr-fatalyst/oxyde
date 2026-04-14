from __future__ import annotations

from module import Category, Item


async def run() -> tuple[list[Category], list[Item]]:
    return await Category.objects.all(), await Item.objects.all()

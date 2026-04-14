from __future__ import annotations

from module import Tagging


async def run() -> list[Tagging]:
    return await Tagging.objects.filter(class_="a", from_="b").all()

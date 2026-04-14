from __future__ import annotations

from module import Pet
from sibling import Owner


async def run() -> tuple[list[Pet], list[Owner]]:
    return await Pet.objects.all(), await Owner.objects.all()

from __future__ import annotations

from module import Payload


async def run() -> list[Payload]:
    return await Payload.objects.all()

from __future__ import annotations

from module import Device


async def run() -> list[Device]:
    return await Device.objects.filter(created_at_epoch__gte=0).all()

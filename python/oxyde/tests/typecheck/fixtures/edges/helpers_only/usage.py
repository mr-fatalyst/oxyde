from __future__ import annotations

from module import compute, fetch_counter


async def run() -> int:
    return compute(await fetch_counter(), 1)

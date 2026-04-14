"""Edge: module has only top-level helpers, no Model. Generator should no-op."""

from __future__ import annotations


async def fetch_counter() -> int:
    return 42


def compute(x: int, y: int) -> int:
    return x + y

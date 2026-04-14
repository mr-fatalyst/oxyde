from __future__ import annotations

from module import Item, resolve


def use() -> tuple[Item, Item | None]:
    return resolve(1), resolve("x")

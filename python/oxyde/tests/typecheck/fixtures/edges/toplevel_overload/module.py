"""Edge: Model + @overload helper."""

from __future__ import annotations

from typing import overload

from oxyde import Field, Model


class Item(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")

    class Meta:
        is_table = True


@overload
def resolve(key: int) -> Item: ...
@overload
def resolve(key: str) -> Item | None: ...
def resolve(key: int | str) -> Item | None:
    _ = key
    return None

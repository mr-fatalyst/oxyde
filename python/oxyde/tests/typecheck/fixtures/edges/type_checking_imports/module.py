"""Edge: TYPE_CHECKING-only import used in annotations alongside a Model."""

from __future__ import annotations

from typing import TYPE_CHECKING

from oxyde import Field, Model

if TYPE_CHECKING:
    from collections.abc import Sequence


class Event(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")

    class Meta:
        is_table = True


def flatten(events: Sequence[Event]) -> list[str]:
    return [e.name for e in events]

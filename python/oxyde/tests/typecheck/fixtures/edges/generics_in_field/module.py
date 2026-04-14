"""Edge: nested generic field annotations (dict of lists, list of tuples)."""

from __future__ import annotations

from typing import Any

from oxyde import Field, Model


class Payload(Model):
    id: int | None = Field(default=None, db_pk=True)
    headers: dict[str, list[str]] = Field(default_factory=dict)
    rows: list[tuple[int, str]] = Field(default_factory=list)
    meta: dict[str, Any] = Field(default_factory=dict)

    class Meta:
        is_table = True

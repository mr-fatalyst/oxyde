"""Minimal model for the negative typecheck fixture."""

from __future__ import annotations

from oxyde import Field, Model


class Item(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")
    qty: int = Field(default=0)

    class Meta:
        is_table = True

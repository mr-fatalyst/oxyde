"""Minimal model for the negative typecheck fixture."""

from __future__ import annotations

from uuid import UUID

from oxyde import Field, Model


class Item(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")
    qty: int = Field(default=0)
    key: UUID | None = Field(default=None)

    class Meta:
        is_table = True

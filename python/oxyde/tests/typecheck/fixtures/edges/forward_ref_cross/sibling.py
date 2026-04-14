"""Sibling module for forward_ref_cross — defines the Owner model."""

from __future__ import annotations

from oxyde import Field, Model


class Owner(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")

    class Meta:
        is_table = True

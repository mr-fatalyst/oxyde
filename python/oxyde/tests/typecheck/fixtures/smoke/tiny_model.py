"""Smoke fixture: minimal model used to prove the typecheck pipeline works."""

from __future__ import annotations

from oxyde import Field, Model


class User(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")
    age: int | None = Field(default=None)

    class Meta:
        is_table = True

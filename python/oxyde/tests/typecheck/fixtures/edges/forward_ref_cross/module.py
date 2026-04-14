"""Edge: FK across two modules. Model module imports from sibling."""

from __future__ import annotations

from oxyde import Field, Model
from sibling import Owner


class Pet(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")
    owner: Owner | None = Field(default=None)

    class Meta:
        is_table = True

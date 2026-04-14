"""Edge: fields whose names collide with Python keywords or Query methods."""

from __future__ import annotations

from oxyde import Field, Model


class Tagging(Model):
    id: int | None = Field(default=None, db_pk=True)
    class_: str = Field(default="", db_column="class")
    from_: str = Field(default="", db_column="from")

    class Meta:
        is_table = True

"""Edge: stub filter params include double-underscore FK traversal lookups."""

from __future__ import annotations

from oxyde import Field, Model


class Author(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")

    class Meta:
        is_table = True


class Post(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str = Field(default="")
    author: Author | None = Field(default=None)

    class Meta:
        is_table = True

"""Edge: stub filter params include the FK companion {field}_id int field."""

from __future__ import annotations

from oxyde import Field, Model


class Tag(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")

    class Meta:
        is_table = True


class Article(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str = Field(default="")
    tag: Tag | None = Field(default=None)

    class Meta:
        is_table = True

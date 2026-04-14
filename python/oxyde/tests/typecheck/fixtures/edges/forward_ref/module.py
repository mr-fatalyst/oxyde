"""Edge: string forward ref to sibling Model in same module."""

from __future__ import annotations

from oxyde import Field, Model


class Category(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")
    items: list[Item] = Field(db_reverse_fk="category_id")

    class Meta:
        is_table = True


class Item(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(default="")
    category: "Category | None" = Field(default=None)

    class Meta:
        is_table = True

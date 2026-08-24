"""Edge: enum columns — one enum defined in the model module itself (the
Django-style layout), one imported from a sibling module. The same-module
enum must be copied into the .pyi: the stub shadows the module, so without
the copy every generated signature referencing it breaks, and so does
``from module import Color`` in user code."""

from __future__ import annotations

from enum import Enum

from oxyde import Field, Model
from sibling import Size


class Color(str, Enum):
    RED = "red"
    GREEN = "green"
    BLUE = "blue"

    def label(self) -> str:
        return self.value.upper()


class Shirt(Model):
    id: int | None = Field(default=None, db_pk=True)
    color: Color = Field(default=Color.RED)
    size: Size = Field(default=Size.M)

    class Meta:
        is_table = True

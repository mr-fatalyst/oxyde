"""Edge: fields whose names collide with Python keywords, Query methods,
or service parameters of generated signatures (client/using/args/kwargs),
plus a class-level @overload method."""

from __future__ import annotations

from typing import overload

from oxyde import Field, Model


class Tagging(Model):
    id: int | None = Field(default=None, db_pk=True)
    class_: str = Field(default="", db_column="class")
    from_: str = Field(default="", db_column="from")
    client: str = Field(default="")
    using: str = Field(default="")
    args: str = Field(default="")
    kwargs: str = Field(default="")
    defaults: str = Field(default="")

    class Meta:
        is_table = True

    @overload
    def render(self, short: bool) -> str: ...
    @overload
    def render(self) -> bytes: ...
    def render(self, short: bool | None = None) -> str | bytes:
        return self.class_ if short else self.class_.encode()

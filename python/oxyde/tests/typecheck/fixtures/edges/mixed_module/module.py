"""Edge: Model alongside module-level helper. AST-merge case — helper must survive."""

from __future__ import annotations

from oxyde import Field, Model


class Note(Model):
    id: int | None = Field(default=None, db_pk=True)
    text: str = Field(default="")

    class Meta:
        is_table = True


async def fetch_recent_notes(limit: int) -> list[Note]:
    return await Note.objects.filter().limit(limit).all()

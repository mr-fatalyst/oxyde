"""Edge: Model + plain dataclass at module level."""

from __future__ import annotations

from dataclasses import dataclass

from oxyde import Field, Model


@dataclass
class SearchCriteria:
    term: str
    limit: int = 20


class Article(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str = Field(default="")

    class Meta:
        is_table = True


async def search(criteria: SearchCriteria) -> list[Article]:
    return (
        await Article.objects.filter(title__icontains=criteria.term)
        .limit(criteria.limit)
        .all()
    )

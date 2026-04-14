from __future__ import annotations

from module import Article, SearchCriteria, search


async def run() -> list[Article]:
    return await search(SearchCriteria(term="x", limit=5))

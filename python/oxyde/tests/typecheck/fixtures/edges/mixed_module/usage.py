from __future__ import annotations

from module import Note, fetch_recent_notes


async def run() -> list[Note]:
    return await fetch_recent_notes(10)

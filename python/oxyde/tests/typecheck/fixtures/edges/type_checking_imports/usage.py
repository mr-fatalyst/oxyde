from __future__ import annotations

from module import Event, flatten


def run(events: list[Event]) -> list[str]:
    return flatten(events)

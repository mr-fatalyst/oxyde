"""Shared test helpers — stubs, factories, utilities."""
from __future__ import annotations

from typing import Any

import msgpack


class StubExecuteClient:
    """Stub that returns pre-configured msgpack payloads.

    Used by unit tests to test query building and result hydration
    without hitting a real database.

    Usage:
        stub = StubExecuteClient(payloads=[{"columns": [...], "rows": [...]}])
        result = await SomeModel.objects.all(client=stub)
        assert stub.calls[0]["operation"] == "select"
    """

    def __init__(self, payloads: list[Any]):
        self.payloads = list(payloads)
        self.calls: list[dict[str, Any]] = []

    async def execute(self, ir: dict[str, Any]) -> bytes:
        self.calls.append(ir)
        if not self.payloads:
            raise RuntimeError("stub payloads exhausted")
        payload = self.payloads.pop(0)
        return payload if isinstance(payload, bytes) else msgpack.packb(payload)

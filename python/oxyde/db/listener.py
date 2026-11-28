"""PostgreSQL notification hints on a dedicated database connection."""

from __future__ import annotations

from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import Any

from oxyde.core import wrapper

from .registry import get_connection


@dataclass(frozen=True)
class Notification:
    """A PostgreSQL notification and the publishing backend's process ID."""

    channel: str
    payload: str
    pid: int


class ListenerClosedError(RuntimeError):
    """A direct receive was attempted on a closed listener."""


class NotificationListener:
    """Single-consumer listener; create with :func:`listen`."""

    def __init__(self, handle: Any) -> None:
        self._handle = handle
        self._receiving = False

    async def _receive(self) -> Notification | None:
        if self._receiving:
            raise RuntimeError("A receive is already pending on this listener")
        self._receiving = True
        try:
            value = await self._handle.recv()
            return None if value is None else Notification(*value)
        finally:
            self._receiving = False

    async def recv(self) -> Notification:
        """Wait for a hint. Raise ListenerClosedError after intentional closure."""
        notification = await self._receive()
        if notification is None:
            raise ListenerClosedError("Notification listener is closed")
        return notification

    def __aiter__(self) -> NotificationListener:
        return self

    async def __anext__(self) -> Notification:
        notification = await self._receive()
        if notification is None:
            raise StopAsyncIteration
        return notification

    async def close(self) -> None:
        """Interrupt pending receives and close the listener; safe to repeat."""
        await self._handle.close()


@asynccontextmanager
async def listen(
    *channels: str, using: str = "default"
) -> AsyncIterator[NotificationListener]:
    """Subscribe before yielding on a dedicated PostgreSQL connection.

    The connection uses the named database's configuration but is outside its
    pool limit. Channels are fixed, unique, nonempty names of at most 63 UTF-8
    bytes with no NUL. Ambient transactions are never used. SQLx restores
    subscriptions after recoverable disconnects, but missed hints are lost:
    subscribe before reading state, then periodically reconcile authoritative
    state. Cancellation of a receive may also discard an in-flight hint.
    """
    if not channels:
        raise ValueError("At least one channel is required")
    for channel in channels:
        if not isinstance(channel, str):
            raise TypeError("Channel names must be strings")
        if not channel or "\0" in channel or len(channel.encode("utf-8")) > 63:
            raise ValueError("Channel names must contain 1–63 UTF-8 bytes and no NUL")
    if len(set(channels)) != len(channels):
        raise ValueError("Duplicate channel names are not allowed")
    database = await get_connection(using)
    handle = await wrapper.listen(database.name, list(channels))
    listener = NotificationListener(handle)
    try:
        yield listener
    finally:
        await listener.close()

"""Smoke usage: exercise a few typed manager/query calls."""

from __future__ import annotations

from tiny_model import User
from typing_extensions import assert_type


async def fetch_adults() -> list[User]:
    return await User.objects.filter(age__gte=18).order_by("-age").all()


async def precise_types() -> None:
    assert_type(await User.objects.all(), list[User])
    assert_type(await User.objects.first(), User | None)


async def create_alice() -> User:
    return await User.objects.create(name="Alice", age=30)


async def get_by_id(user_id: int) -> User | None:
    return await User.objects.get_or_none(id=user_id)

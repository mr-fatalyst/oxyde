"""Enum usage: import the same-module enum THROUGH the stub, filter by enum
values, and pin down the field types with assert_type."""

from __future__ import annotations

from module import Color, Shirt
from sibling import Size
from typing_extensions import assert_type


async def filter_by_enums() -> list[Shirt]:
    return await Shirt.objects.filter(
        color=Color.RED,
        size__in=[Size.S, Size.M],
        color__isnull=False,
    ).all()


async def exclude_by_enum() -> list[Shirt]:
    return await Shirt.objects.exclude(size=Size.L).all()


async def create_with_enums() -> Shirt:
    return await Shirt.objects.create(color=Color.BLUE, size=Size.S)


def enum_fields(shirt: Shirt) -> None:
    assert_type(shirt.color, Color)
    assert_type(shirt.size, Size)
    assert_type(shirt.color.label(), str)

"""Intentionally WRONG usage. Every line marked ``# type-error`` must be
flagged by the checker; unmarked lines must stay clean. This is the guard
against stubs degrading to permissive Any-signatures: a stub that accepts
everything would pass every positive fixture and fail here.

Keep each wrong call on a single line — checkers differ in which physical
line of a multi-line call they attach the diagnostic to.
"""

from __future__ import annotations

from module import Item


async def wrong() -> None:
    await Item.objects.filter(name=5).all()  # type-error
    await Item.objects.filter(qty__gte="high").all()  # type-error
    await Item.objects.filter(qty__in="abc").all()  # type-error
    await Item.objects.order_by(123).all()  # type-error
    Item(name=5)  # type-error
    print((await Item.objects.first()).name)  # type-error
    rows: list[Item] = await Item.objects.filter(id=1).update(name="x")  # type-error
    print(rows)

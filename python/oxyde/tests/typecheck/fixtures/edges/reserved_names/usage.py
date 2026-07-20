from __future__ import annotations

from module import Tagging


async def run() -> list[Tagging]:
    return await Tagging.objects.filter(class_="a", from_="b").all()


async def reserved_fields_via_kwargs() -> list[Tagging]:
    # Fields named client/using/args/kwargs/defaults collide with service
    # parameters, so they are not enumerated in stub signatures — but they
    # must still be accepted (runtime filters by them via **kwargs).
    return await Tagging.objects.filter(client="x", using="y", defaults="z").all()


def reserved_fields_as_attributes(t: Tagging) -> tuple[str, str, str, str, str]:
    return t.client, t.using, t.args, t.kwargs, t.defaults


def overloaded_method(t: Tagging) -> tuple[str, bytes]:
    return t.render(True), t.render()

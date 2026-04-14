"""Edge: concrete Model inherits from a non-table intermediate class."""

from __future__ import annotations

from oxyde import Field, Model


class TimestampedBase(Model):
    """Non-table base providing shared field — must not appear in registry."""

    created_at_epoch: int = Field(default=0)


class Device(TimestampedBase):
    id: int | None = Field(default=None, db_pk=True)
    serial: str = Field(default="")

    class Meta:
        is_table = True

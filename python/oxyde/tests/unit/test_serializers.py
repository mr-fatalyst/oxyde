"""Tests for INSERT/UPDATE serialization."""

from __future__ import annotations

from datetime import datetime
from uuid import UUID, uuid4

from oxyde import Field, Model
from oxyde.models.registry import registered_tables
from oxyde.models.serializers import _dump_insert_data


class TestInsertDefaults:
    """Values from default=/default_factory must reach the INSERT payload."""

    def test_default_factory_included(self):
        """default_factory value is on the instance and must be inserted."""

        class FactoryAuthor(Model):
            id: UUID | None = Field(default_factory=uuid4, db_pk=True)
            name: str

            class Meta:
                is_table = True

        registered_tables()

        author = FactoryAuthor(name="Alice")
        data = _dump_insert_data(author)

        assert data["name"] == "Alice"
        assert isinstance(data["id"], UUID)
        assert data["id"] == author.id

    def test_plain_default_included(self):
        """status: str = Field(default="active") must insert "active"."""

        class DefaultedItem(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str
            status: str = Field(default="active")
            qty: int = Field(default=0)

            class Meta:
                is_table = True

        registered_tables()

        data = _dump_insert_data(DefaultedItem(name="x"))

        assert data["status"] == "active"
        assert data["qty"] == 0

    def test_unset_none_pk_omitted_for_autoincrement(self):
        """Untouched Field(default=None, db_pk=True) stays out of the INSERT
        so the database sequence fills it."""

        class SeqItem(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True

        registered_tables()

        data = _dump_insert_data(SeqItem(name="x"))

        assert "id" not in data
        assert data == {"name": "x"}

    def test_explicit_none_is_sent(self):
        """field=None passed explicitly must reach the database as NULL
        (e.g. to override a server-side default)."""

        class NullableNote(Model):
            id: int | None = Field(default=None, db_pk=True)
            body: str
            reviewed_at: datetime | None = Field(
                default_factory=lambda: datetime(2026, 1, 1)
            )

            class Meta:
                is_table = True

        registered_tables()

        data = _dump_insert_data(NullableNote(body="x", reviewed_at=None))

        assert "reviewed_at" in data
        assert data["reviewed_at"] is None

    def test_unset_none_regular_field_omitted(self):
        """A nullable non-pk field left at its None default is omitted, so a
        server-side default (db_default) can fill it."""

        class ServerDefaulted(Model):
            id: int | None = Field(default=None, db_pk=True)
            body: str
            created_at: datetime | None = Field(default=None, db_default="NOW()")

            class Meta:
                is_table = True

        registered_tables()

        data = _dump_insert_data(ServerDefaulted(body="x"))

        assert "created_at" not in data

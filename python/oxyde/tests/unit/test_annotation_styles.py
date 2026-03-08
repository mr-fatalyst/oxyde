"""Tests for FK/relation detection across different Python annotation styles.

Verifies that Oxyde correctly detects FK fields regardless of how
the type annotation is written (direct type, Optional, forward ref, etc.).

NOTE: This file intentionally does NOT use `from __future__ import annotations`
at module level — some tests need eager annotation evaluation to verify
real-world behavior. Tests that require lazy annotations define models
inside exec() with the future import.
"""

from typing import Optional

import pytest

from oxyde import Field, Model
from oxyde.models.registry import registered_tables


# ─── FK: direct type (Author defined above) ──────────────────────────


class _FKDirectAuthor(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str

    class Meta:
        is_table = True


class _FKDirectBook(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: _FKDirectAuthor | None = Field(default=None, db_on_delete="CASCADE")

    class Meta:
        is_table = True


class TestFKDirectType:
    def test_fk_detected(self):
        registered_tables()
        assert "author_id" in _FKDirectBook.model_fields
        meta = _FKDirectBook._db_meta.field_metadata["author"]
        assert meta.foreign_key is not None
        assert meta.foreign_key.column_name == "author_id"


# ─── FK: Optional[Author] (direct type) ──────────────────────────────


class _FKOptAuthor(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str

    class Meta:
        is_table = True


class _FKOptBook(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: Optional[_FKOptAuthor] = Field(default=None, db_on_delete="CASCADE")

    class Meta:
        is_table = True


class TestFKOptionalDirectType:
    def test_fk_detected(self):
        registered_tables()
        assert "author_id" in _FKOptBook.model_fields
        meta = _FKOptBook._db_meta.field_metadata["author"]
        assert meta.foreign_key is not None
        assert meta.foreign_key.column_name == "author_id"


# ─── FK: Optional["Author"] (ForwardRef) ─────────────────────────────


class _FKOptStrAuthor(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str

    class Meta:
        is_table = True


class _FKOptStrBook(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: Optional["_FKOptStrAuthor"] = Field(
        default=None, db_on_delete="CASCADE"
    )

    class Meta:
        is_table = True


class TestFKOptionalStringRef:
    @pytest.mark.xfail(
        reason="ForwardRef from Optional['Model'] not handled in metaclass yet"
    )
    def test_fk_detected(self):
        registered_tables()
        assert "author_id" in _FKOptStrBook.model_fields
        meta = _FKOptStrBook._db_meta.field_metadata["author"]
        assert meta.foreign_key is not None
        assert meta.foreign_key.column_name == "author_id"


# ─── FK: from __future__ import annotations + direct type ────────────


class TestFKFutureAnnotations:
    def test_fk_detected(self):
        ns = {}
        exec(
            """
from __future__ import annotations
from oxyde import Field, Model
from oxyde.models.registry import registered_tables

class FutAuthor(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    class Meta:
        is_table = True

class FutBook(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: FutAuthor | None = Field(default=None, db_on_delete="CASCADE")
    class Meta:
        is_table = True

registered_tables()
result_fields = list(FutBook.model_fields.keys())
result_fk = FutBook._db_meta.field_metadata["author"].foreign_key
""",
            ns,
        )
        assert "author_id" in ns["result_fields"]
        assert ns["result_fk"] is not None
        assert ns["result_fk"].column_name == "author_id"


# ─── FK: self-referential + from __future__ import annotations ───────


class TestFKSelfReferential:
    def test_fk_detected(self):
        ns = {}
        exec(
            """
from __future__ import annotations
from oxyde import Field, Model
from oxyde.models.registry import registered_tables

class SelfRefCategory(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    parent: SelfRefCategory | None = Field(default=None, db_on_delete="SET NULL")
    class Meta:
        is_table = True

registered_tables()
result_fields = list(SelfRefCategory.model_fields.keys())
result_fk = SelfRefCategory._db_meta.field_metadata["parent"].foreign_key
""",
            ns,
        )
        assert "parent_id" in ns["result_fields"]
        assert ns["result_fk"] is not None
        assert ns["result_fk"].column_name == "parent_id"


# ─── Reverse FK: list[Post] (direct type, defined above) ─────────────


class _RevPost(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str

    class Meta:
        is_table = True


class _RevAuthor(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    posts: list[_RevPost] = Field(db_reverse_fk="author")

    class Meta:
        is_table = True


class TestReverseFKDirectType:
    def test_reverse_fk_field_exists(self):
        registered_tables()
        assert "posts" in _RevAuthor.model_fields


# ─── Reverse FK: list["Post"] (string forward ref) ───────────────────


class _RevStrAuthor(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    posts: list["_RevStrPost"] = Field(db_reverse_fk="author")

    class Meta:
        is_table = True


class _RevStrPost(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: _RevStrAuthor | None = Field(default=None, db_on_delete="CASCADE")

    class Meta:
        is_table = True


class TestReverseFKStringRef:
    def test_reverse_fk_field_exists(self):
        registered_tables()
        assert "posts" in _RevStrAuthor.model_fields


# ─── Reverse FK: from __future__ + list[Post] ────────────────────────


class TestReverseFKFutureAnnotations:
    def test_reverse_fk_field_exists(self):
        ns = {}
        exec(
            """
from __future__ import annotations
from oxyde import Field, Model
from oxyde.models.registry import registered_tables

class FutRevAuthor(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str
    posts: list[FutRevPost] = Field(db_reverse_fk="author")
    class Meta:
        is_table = True

class FutRevPost(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str
    author: FutRevAuthor | None = Field(default=None, db_on_delete="CASCADE")
    class Meta:
        is_table = True

registered_tables()
result_fields = list(FutRevAuthor.model_fields.keys())
""",
            ns,
        )
        assert "posts" in ns["result_fields"]

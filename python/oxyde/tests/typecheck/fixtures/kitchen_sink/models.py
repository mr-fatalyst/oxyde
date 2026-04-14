"""Kitchen-sink fixture: all supported field types, PK variants, FK, reverse FK, M2M."""

from __future__ import annotations

from datetime import date, datetime, time
from decimal import Decimal
from typing import Any, Literal
from uuid import UUID, uuid4

from oxyde import Field, Model


class Author(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(max_length=100)
    email: str = Field(db_unique=True, db_index=True)
    active: bool = Field(default=True)
    posts: list[Post] = Field(db_reverse_fk="author_id")

    class Meta:
        is_table = True
        table_name = "authors"


class Tag(Model):
    id: int | None = Field(default=None, db_pk=True)
    name: str = Field(max_length=50, db_unique=True)

    class Meta:
        is_table = True
        table_name = "tags"


class PostTag(Model):
    id: int | None = Field(default=None, db_pk=True)
    post: Post | None = Field(default=None)
    tag: Tag | None = Field(default=None)

    class Meta:
        is_table = True
        table_name = "post_tags"


class Post(Model):
    id: int | None = Field(default=None, db_pk=True)
    title: str = Field(max_length=200, db_index=True)
    body: str = Field(default="")
    views: int = Field(default=0)
    score: float = Field(default=0.0)
    price: Decimal = Field(default=Decimal("0.00"))
    published: bool = Field(default=False)
    data: bytes | None = Field(default=None)
    created_at: datetime = Field(db_default="NOW()")
    published_on: date | None = Field(default=None)
    publish_time: time | None = Field(default=None)
    slug_id: UUID = Field(default_factory=uuid4)
    status: Literal["draft", "published", "archived"] = Field(default="draft")
    metadata: dict[str, Any] = Field(default_factory=dict)
    labels: list[str] = Field(default_factory=list)

    author: Author | None = Field(default=None, db_on_delete="CASCADE")
    tags: list[Tag] = Field(db_m2m=True, db_through="PostTag")

    class Meta:
        is_table = True
        table_name = "posts"
        schema = "public"

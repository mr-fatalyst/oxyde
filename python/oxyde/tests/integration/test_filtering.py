"""Integration tests for filtering and Q expressions."""

from __future__ import annotations

from datetime import datetime

import pytest

from oxyde import Q

from .conftest import AliasedEvent, Author, Event, Post


class TestBasicFilters:
    @pytest.mark.asyncio
    async def test_filter_exact(self, db):
        authors = await Author.objects.filter(name="Alice").all(using=db.name)
        assert len(authors) == 1
        assert authors[0].email == "alice@test.com"

    @pytest.mark.asyncio
    async def test_filter_iexact(self, db):
        authors = await Author.objects.filter(name__iexact="alice").all(using=db.name)
        assert len(authors) == 1
        assert authors[0].name == "Alice"

    @pytest.mark.asyncio
    async def test_filter_contains(self, db):
        posts = await Post.objects.filter(title__contains="Python").all(using=db.name)
        assert len(posts) == 1
        assert posts[0].title == "Async Python"

    @pytest.mark.asyncio
    async def test_filter_icontains(self, db):
        posts = await Post.objects.filter(title__icontains="python").all(using=db.name)
        assert len(posts) == 1
        assert posts[0].title == "Async Python"

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        ("lookup", "value"),
        [
            ("contains", "snake_case"),
            ("icontains", "SNAKE_CASE"),
            ("startswith", "snake_case"),
            ("istartswith", "SNAKE_CASE"),
            ("endswith", "case_title"),
            ("iendswith", "CASE_TITLE"),
        ],
    )
    async def test_pattern_lookups_match_literal_underscore(self, db, lookup, value):
        await Post.objects.create(
            title="snake_case_title",
            body="",
            author_id=1,
            category_id=1,
            views=1,
            published=True,
            using=db.name,
        )

        posts = await Post.objects.filter(**{f"title__{lookup}": value}).all(
            using=db.name
        )

        assert len(posts) == 1
        assert posts[0].title == "snake_case_title"


class TestNumericFilters:
    @pytest.mark.asyncio
    async def test_filter_gt(self, db):
        posts = await Post.objects.filter(views__gt=100).all(using=db.name)
        assert len(posts) == 2  # Rust Patterns (120), ML Basics (200)

    @pytest.mark.asyncio
    async def test_filter_gte(self, db):
        posts = await Post.objects.filter(views__gte=120).all(using=db.name)
        assert len(posts) == 2  # Rust Patterns (120), ML Basics (200)

    @pytest.mark.asyncio
    async def test_filter_lt(self, db):
        posts = await Post.objects.filter(views__lt=35).all(using=db.name)
        # views=0 (Draft Post, Unpublished)
        assert len(posts) == 2

    @pytest.mark.asyncio
    async def test_filter_lte(self, db):
        posts = await Post.objects.filter(views__lte=35).all(using=db.name)
        # views=0 (x2), views=35 (Async Python)
        assert len(posts) == 3

    @pytest.mark.asyncio
    async def test_filter_in(self, db):
        posts = await Post.objects.filter(id__in=[1, 3, 5]).all(using=db.name)
        assert len(posts) == 3
        titles = {p.title for p in posts}
        assert titles == {"Rust Patterns", "Quantum Computing", "ML Basics"}

    @pytest.mark.asyncio
    async def test_filter_isnull_true(self, db):
        posts = await Post.objects.filter(category_id__isnull=True).all(using=db.name)
        assert len(posts) == 2  # Draft Post, Unpublished

    @pytest.mark.asyncio
    async def test_filter_isnull_false(self, db):
        posts = await Post.objects.filter(category_id__isnull=False).all(using=db.name)
        assert len(posts) == 4

    @pytest.mark.asyncio
    async def test_filter_between(self, db):
        posts = await Post.objects.filter(views__between=(35, 120)).all(using=db.name)
        titles = {p.title for p in posts}
        assert titles == {"Async Python", "Quantum Computing", "Rust Patterns"}


class TestExclude:
    @pytest.mark.asyncio
    async def test_exclude(self, db):
        authors = await Author.objects.exclude(active=False).all(using=db.name)
        assert len(authors) == 2
        names = {a.name for a in authors}
        assert names == {"Alice", "Bob"}


class TestQExpressions:
    @pytest.mark.asyncio
    async def test_q_and(self, db):
        posts = await Post.objects.filter(Q(published=True) & Q(views__gte=100)).all(
            using=db.name
        )
        assert len(posts) == 2  # Rust Patterns (120), ML Basics (200)

    @pytest.mark.asyncio
    async def test_q_or(self, db):
        posts = await Post.objects.filter(Q(views__gte=200) | Q(views=0)).all(
            using=db.name
        )
        assert len(posts) == 3  # ML Basics, Draft Post, Unpublished

    @pytest.mark.asyncio
    async def test_q_not(self, db):
        authors = await Author.objects.filter(~Q(active=False)).all(using=db.name)
        assert len(authors) == 2
        names = {a.name for a in authors}
        assert names == {"Alice", "Bob"}

    @pytest.mark.asyncio
    async def test_q_nested(self, db):
        """(published OR views > 100) AND NOT author_id=3"""
        posts = await Post.objects.filter(
            (Q(published=True) | Q(views__gt=100)) & ~Q(author_id=3)
        ).all(using=db.name)
        titles = {p.title for p in posts}
        assert titles == {"Rust Patterns", "Async Python", "Quantum Computing"}


class TestFKTraversal:
    @pytest.mark.asyncio
    async def test_filter_fk_traversal(self, db):
        """Filter posts by author's name via FK join."""
        posts = await Post.objects.filter(author__name="Alice").all(using=db.name)
        assert len(posts) == 2
        titles = {p.title for p in posts}
        assert titles == {"Rust Patterns", "Async Python"}


class TestChaining:
    @pytest.mark.asyncio
    async def test_filter_chaining(self, db):
        """Chained .filter() calls should AND conditions."""
        posts_chained = await (
            Post.objects.filter(published=True)
            .filter(views__gte=100)
            .all(using=db.name)
        )
        posts_single = await Post.objects.filter(published=True, views__gte=100).all(
            using=db.name
        )
        assert len(posts_chained) == len(posts_single)
        assert {p.id for p in posts_chained} == {p.id for p in posts_single}


class TestDatetimeFilters:
    @pytest.mark.asyncio
    async def test_datetime_gte_includes_same_day_with_time(self, event_db):
        """gte midnight should include same-day records with time > 00:00."""
        events = await Event.objects.filter(
            created_at__gte=datetime(2026, 3, 14),
        ).all(using=event_db.name)
        assert len(events) == 5

    @pytest.mark.asyncio
    async def test_datetime_range_includes_same_day(self, event_db):
        """Range filter should include all records within the range."""
        events = await Event.objects.filter(
            created_at__gte=datetime(2026, 3, 14),
            created_at__lt=datetime(2026, 3, 17),
        ).all(using=event_db.name)
        assert len(events) == 5

    @pytest.mark.asyncio
    async def test_datetime_gte_with_time_excludes_earlier(self, event_db):
        """gte with time 10:00 should exclude records at 09:34."""
        events = await Event.objects.filter(
            created_at__gte=datetime(2026, 3, 14, 10, 0, 0),
        ).all(using=event_db.name)
        assert len(events) == 2  # Midnight + Next Day

    @pytest.mark.asyncio
    async def test_datetime_lt_excludes_later(self, event_db):
        """lt midnight of a day should exclude that day's records."""
        events = await Event.objects.filter(
            created_at__lt=datetime(2026, 3, 15),
        ).all(using=event_db.name)
        assert len(events) == 3  # Morning A, B, C

    @pytest.mark.asyncio
    async def test_datetime_gt_excludes_equal(self, event_db):
        """gt specific time should exclude records at that exact time."""
        events = await Event.objects.filter(
            created_at__gt=datetime(2026, 3, 14, 9, 34, 18),
        ).all(using=event_db.name)
        assert len(events) == 2  # Midnight + Next Day

    @pytest.mark.asyncio
    async def test_datetime_lte_includes_equal(self, event_db):
        """lte specific time should include records at that exact time."""
        events = await Event.objects.filter(
            created_at__lte=datetime(2026, 3, 14, 9, 34, 18),
        ).all(using=event_db.name)
        assert len(events) == 3  # Morning A, B, C


class TestDbColumnAliasing:
    """fields with db_column != field_name must work correctly."""

    @pytest.mark.asyncio
    async def test_select_with_aliased_columns(self, aliased_db):
        """SELECT should return field_names, not db_columns."""
        events = await AliasedEvent.objects.all(using=aliased_db.name)
        assert len(events) == 2
        assert events[0].title == "Morning A"
        assert isinstance(events[0].created, datetime)

    @pytest.mark.asyncio
    async def test_filter_with_aliased_datetime(self, aliased_db):
        """datetime filter on aliased field should use correct type hint."""
        events = await AliasedEvent.objects.filter(
            created__gte=datetime(2026, 3, 15),
        ).all(using=aliased_db.name)
        assert len(events) == 1
        assert events[0].title == "Midnight"

    @pytest.mark.asyncio
    async def test_create_with_aliased_columns(self, aliased_db):
        """RETURNING from create should remap db_columns to field_names."""
        event = await AliasedEvent.objects.create(
            title="New Event",
            created=datetime(2026, 4, 1, 12, 0, 0),
            using=aliased_db.name,
        )
        assert event.title == "New Event"
        assert event.created == datetime(2026, 4, 1, 12, 0, 0)

    @pytest.mark.asyncio
    async def test_bulk_update_with_aliased_columns(self, aliased_db):
        """bulk_update must map field names to db_column."""
        events = await AliasedEvent.objects.all(using=aliased_db.name)
        assert len(events) == 2

        events[0].title = "Updated Morning"
        events[1].title = "Updated Midnight"

        affected = await AliasedEvent.objects.bulk_update(
            events, ["title"], using=aliased_db.name
        )
        assert affected == 2

        refreshed = await AliasedEvent.objects.order_by("id").all(using=aliased_db.name)
        assert refreshed[0].title == "Updated Morning"
        assert refreshed[1].title == "Updated Midnight"

    @pytest.mark.asyncio
    async def test_update_with_aliased_columns(self, aliased_db):
        """queryset update() must map field names to db_column."""
        affected = await AliasedEvent.objects.filter(title="Morning A").update(
            title="Morning B", using=aliased_db.name
        )
        assert affected == 1
        event = await AliasedEvent.objects.get(title="Morning B", using=aliased_db.name)
        assert event.title == "Morning B"

    @pytest.mark.asyncio
    async def test_delete_with_aliased_columns(self, aliased_db):
        """delete() with filter on aliased field must use db_column."""
        affected = await AliasedEvent.objects.filter(title="Morning A").delete(
            using=aliased_db.name
        )
        assert affected == 1
        remaining = await AliasedEvent.objects.all(using=aliased_db.name)
        assert len(remaining) == 1

    @pytest.mark.asyncio
    async def test_save_update_with_aliased_columns(self, aliased_db):
        """instance.save() on existing record must use db_column."""
        event = await AliasedEvent.objects.get(title="Morning A", using=aliased_db.name)
        event.title = "Morning Saved"
        await event.save(using=aliased_db.name)

        refreshed = await AliasedEvent.objects.get(id=event.id, using=aliased_db.name)
        assert refreshed.title == "Morning Saved"

    @pytest.mark.asyncio
    async def test_update_or_create_with_aliased_columns(self, aliased_db):
        """update_or_create() on an existing row must use aliased db_column names."""
        existing = await AliasedEvent.objects.get(
            title="Morning A", using=aliased_db.name
        )

        event, created = await AliasedEvent.objects.update_or_create(
            id=existing.id,
            defaults={"title": "Morning Updated"},
            using=aliased_db.name,
        )

        assert created is False
        assert event.title == "Morning Updated"

        refreshed = await AliasedEvent.objects.get(
            id=existing.id, using=aliased_db.name
        )
        assert refreshed.title == "Morning Updated"

    @pytest.mark.asyncio
    async def test_bulk_create_with_aliased_columns(self, aliased_db):
        """bulk_create must map field names to db_column."""
        new_events = [
            AliasedEvent(title="Bulk A", created=datetime(2026, 5, 1)),
            AliasedEvent(title="Bulk B", created=datetime(2026, 5, 2)),
        ]
        created = await AliasedEvent.objects.bulk_create(
            new_events, using=aliased_db.name
        )
        assert len(created) == 2

        all_events = await AliasedEvent.objects.all(using=aliased_db.name)
        assert len(all_events) == 4

    @pytest.mark.asyncio
    async def test_order_by_with_aliased_columns(self, aliased_db):
        """order_by on aliased field must use db_column."""
        events = await AliasedEvent.objects.order_by("-created").all(
            using=aliased_db.name
        )
        assert events[0].title == "Midnight"
        assert events[1].title == "Morning A"

    @pytest.mark.asyncio
    async def test_values_with_aliased_columns(self, aliased_db):
        """values() must return field names, not db_columns."""
        rows = (
            await AliasedEvent.objects.order_by("id")
            .values("id", "title")
            .all(using=aliased_db.name)
        )
        assert len(rows) == 2
        assert rows[0]["title"] == "Morning A"
        assert "event_title" not in rows[0]

    @pytest.mark.asyncio
    async def test_values_list_with_aliased_columns(self, aliased_db):
        """values_list() must work with field names."""
        rows = (
            await AliasedEvent.objects.order_by("id")
            .values_list("title", flat=True)
            .all(using=aliased_db.name)
        )
        assert rows == ["Morning A", "Midnight"]

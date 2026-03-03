"""Integration tests for transactions, savepoints, and rollback."""
from __future__ import annotations

import pytest

from oxyde import atomic, execute_raw

from .conftest import Author


class TestAtomic:
    @pytest.mark.asyncio
    async def test_atomic_commit(self, db):
        """Data persists after successful atomic block."""
        async with atomic(using=db.name):
            await Author.objects.create(
                name="TxAuthor", email="tx@test.com", using=db.name
            )

        authors = await Author.objects.filter(name="TxAuthor").all(client=db)
        assert len(authors) == 1
        assert authors[0].email == "tx@test.com"

    @pytest.mark.asyncio
    async def test_atomic_rollback(self, db):
        """Data is rolled back when exception is raised."""
        with pytest.raises(ValueError, match="force rollback"):
            async with atomic(using=db.name):
                await Author.objects.create(
                    name="RollbackAuthor", email="rb@test.com", using=db.name
                )
                raise ValueError("force rollback")

        authors = await Author.objects.filter(name="RollbackAuthor").all(client=db)
        assert len(authors) == 0

    @pytest.mark.asyncio
    async def test_set_rollback(self, db):
        """set_rollback() forces rollback without raising an exception."""
        async with atomic(using=db.name) as ctx:
            await Author.objects.create(
                name="ForcedRollback", email="fr@test.com", using=db.name
            )
            ctx.set_rollback()

        authors = await Author.objects.filter(name="ForcedRollback").all(client=db)
        assert len(authors) == 0


class TestNestedTransaction:
    @pytest.mark.asyncio
    async def test_nested_savepoint(self, db):
        """Inner atomic rolls back to savepoint, outer commits."""
        async with atomic(using=db.name):
            await Author.objects.create(
                name="Outer", email="outer@test.com", using=db.name
            )
            try:
                async with atomic(using=db.name):
                    await Author.objects.create(
                        name="Inner", email="inner@test.com", using=db.name
                    )
                    raise ValueError("inner fails")
            except ValueError:
                pass

        outer = await Author.objects.filter(name="Outer").all(client=db)
        assert len(outer) == 1

        inner = await Author.objects.filter(name="Inner").all(client=db)
        assert len(inner) == 0


class TestRawInTransaction:
    @pytest.mark.asyncio
    async def test_execute_raw_in_transaction(self, db):
        """execute_raw participates in the active transaction."""
        async with atomic(using=db.name):
            await execute_raw(
                "INSERT INTO authors (name, email, active) VALUES (?, ?, ?)",
                ["RawTx", "rawtx@test.com", 1],
                using=db.name,
            )

        authors = await Author.objects.filter(name="RawTx").all(client=db)
        assert len(authors) == 1
        assert authors[0].email == "rawtx@test.com"

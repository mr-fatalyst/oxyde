"""Oxyde ORM Transactions Example.

Demonstrates transaction handling:
- atomic() context manager for transactions
- Automatic commit on success
- Automatic rollback on exception
- Nested transactions via savepoints
- Timeout handling

Usage:
    export DATABASE_URL=sqlite://demo.db
    python examples/transactions.py
"""

from __future__ import annotations

import asyncio
import os

from oxyde import AsyncDatabase, OxydeModel, Field, F, atomic, disconnect_all, execute_raw
from oxyde.db.transaction import TransactionTimeoutError


# =============================================================================
# Model Definitions
# =============================================================================

class Account(OxydeModel):
    """Bank account model for transaction demo."""

    id: int | None = Field(default=None, db_pk=True)
    owner: str
    balance: int  # in cents

    class Meta:
        is_table = True
        table_name = "accounts"


# =============================================================================
# Database Setup
# =============================================================================

async def setup_tables(db: AsyncDatabase) -> None:
    """Create tables using raw SQL."""
    await execute_raw("""
        CREATE TABLE IF NOT EXISTS accounts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            owner TEXT NOT NULL,
            balance INTEGER NOT NULL DEFAULT 0
        )
    """, using=db.name)


async def seed_data(db: AsyncDatabase) -> list[Account]:
    """Insert sample accounts."""
    await Account.objects.filter().delete(using=db.name)

    accounts = await Account.objects.bulk_create(
        [
            {"owner": "Alice", "balance": 100000},  # $1000
            {"owner": "Bob", "balance": 50000},     # $500
            {"owner": "Charlie", "balance": 25000}, # $250
        ],
        using=db.name,
    )
    return accounts


async def print_balances(db: AsyncDatabase) -> None:
    """Print current account balances."""
    accounts = await Account.objects.order_by("owner").all(using=db.name)
    for acc in accounts:
        print(f"      {acc.owner}: ${acc.balance / 100:.2f}")


# =============================================================================
# Transaction Examples
# =============================================================================

async def transfer_funds(
    db: AsyncDatabase,
    from_id: int,
    to_id: int,
    amount: int,
) -> None:
    """Transfer funds between accounts atomically.

    Uses F() expressions for atomic field updates.
    """
    async with atomic(using=db.name) as tx:
        # Debit from source account (atomic decrement)
        await Account.objects.filter(id=from_id).update(
            balance=F("balance") - amount,
            client=tx,
        )

        # Credit to destination account (atomic increment)
        await Account.objects.filter(id=to_id).update(
            balance=F("balance") + amount,
            client=tx,
        )


async def demo_successful_transaction(db: AsyncDatabase, accounts: list[Account]) -> None:
    """Demo: Successful transaction commits automatically."""
    print("\n1. Successful Transaction")
    print("   Transferring $100 from Alice to Bob...")

    async with atomic(using=db.name) as tx:
        # Debit Alice
        await Account.objects.filter(id=accounts[0].id).update(
            balance=accounts[0].balance - 10000,
            client=tx,
        )
        # Credit Bob
        await Account.objects.filter(id=accounts[1].id).update(
            balance=accounts[1].balance + 10000,
            client=tx,
        )
    # Transaction commits automatically when exiting context

    print("   Transaction committed!")
    print("   Balances after:")
    await print_balances(db)


async def demo_failed_transaction(db: AsyncDatabase, accounts: list[Account]) -> None:
    """Demo: Failed transaction rolls back automatically."""
    print("\n2. Failed Transaction (Rollback)")
    print("   Attempting transfer that will fail...")

    try:
        async with atomic(using=db.name) as tx:
            # Debit Bob
            await Account.objects.filter(id=accounts[1].id).update(
                balance=accounts[1].balance - 5000,
                client=tx,
            )

            print("   Bob debited, now simulating error...")
            raise ValueError("Simulated error during transfer!")

            # This never executes
            await Account.objects.filter(id=accounts[2].id).update(
                balance=accounts[2].balance + 5000,
                client=tx,
            )

    except ValueError as e:
        print(f"   Caught error: {e}")
        print("   Transaction rolled back automatically!")

    print("   Balances after (unchanged from Bob's debit):")
    await print_balances(db)


async def demo_nested_transactions(db: AsyncDatabase, accounts: list[Account]) -> None:
    """Demo: Nested transactions use savepoints."""
    print("\n3. Nested Transactions (Savepoints)")
    print("   Outer transaction with inner savepoint...")

    async with atomic(using=db.name) as outer_tx:
        # First operation in outer transaction
        await Account.objects.filter(id=accounts[0].id).update(
            balance=accounts[0].balance - 2000,  # -$20 from Alice
            client=outer_tx,
        )
        print("   Outer: Alice debited $20")

        try:
            async with atomic(using=db.name) as inner_tx:
                # This will be in a savepoint
                await Account.objects.filter(id=accounts[2].id).update(
                    balance=accounts[2].balance + 2000,  # +$20 to Charlie
                    client=inner_tx,
                )
                print("   Inner: Charlie credited $20")

                # Simulate failure in inner transaction
                raise RuntimeError("Inner transaction failed!")

        except RuntimeError:
            print("   Inner transaction rolled back to savepoint")

        # Outer transaction can still continue and commit
        # But Charlie's credit is rolled back
        await Account.objects.filter(id=accounts[1].id).update(
            balance=accounts[1].balance + 2000,  # +$20 to Bob instead
            client=outer_tx,
        )
        print("   Outer: Bob credited $20 instead")

    print("   Outer transaction committed!")
    print("   Balances after:")
    await print_balances(db)


# =============================================================================
# Main Demo
# =============================================================================

async def main() -> None:
    url = os.getenv("DATABASE_URL", "sqlite://demo.db")
    db = AsyncDatabase(url, name="default")
    await db.connect()

    try:
        await setup_tables(db)
        accounts = await seed_data(db)

        print("=" * 60)
        print("Transactions Demo")
        print("=" * 60)
        print("\nInitial balances:")
        await print_balances(db)

        # Demo 1: Successful transaction
        await demo_successful_transaction(db, accounts)

        # Refresh account data
        accounts = list(await Account.objects.order_by("id").all(using=db.name))

        # Demo 2: Failed transaction with rollback
        await demo_failed_transaction(db, accounts)

        # Refresh account data
        accounts = list(await Account.objects.order_by("id").all(using=db.name))

        # Demo 3: Nested transactions
        await demo_nested_transactions(db, accounts)

        print("\n" + "=" * 60)
        print("Done!")
        print("=" * 60)

    finally:
        await disconnect_all()


if __name__ == "__main__":
    asyncio.run(main())

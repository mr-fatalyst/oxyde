# Transactions

Oxyde provides Django-style transaction management with the `transaction.atomic()` context manager.

## Basic Usage

```python
from oxyde.db import transaction

async with transaction.atomic():
    user = await User.objects.create(name="Alice")
    await Profile.objects.create(user_id=user.id)
    # Commits automatically on exit
```

If an exception occurs, the transaction rolls back:

```python
async with transaction.atomic():
    user = await User.objects.create(name="Alice")
    raise ValueError("Something went wrong")
    # Transaction rolled back - user not created
```

## Nested Transactions (Savepoints)

Nested `transaction.atomic()` blocks create savepoints:

```python
async with transaction.atomic():
    user = await User.objects.create(name="Alice")

    try:
        async with transaction.atomic():  # Creates savepoint
            await Post.objects.create(author_id=user.id, title="Test")
            raise ValueError("Rollback inner only")
    except ValueError:
        pass  # Inner transaction rolled back

    # User still created - only post was rolled back
    # Outer transaction commits
```

## Manual Rollback

Force rollback without an exception:

```python
async with transaction.atomic() as tx:
    await User.objects.create(name="Alice")

    if some_condition:
        tx.set_rollback(True)  # Mark for rollback
    # Transaction will rollback even without exception
```

## Transaction Timeout

Set a timeout for long-running transactions:

```python
from oxyde.db import transaction, TransactionTimeoutError

try:
    async with transaction.atomic(timeout=30):  # 30 seconds
        await slow_operation()
except TransactionTimeoutError:
    print("Transaction timed out")
```

## Specifying Database

Use transactions on a specific database:

```python
async with transaction.atomic(using="analytics"):
    await Event.objects.create(type="signup")
```

## Row Locking

Lock rows for update within a transaction:

```python
async with transaction.atomic():
    # SELECT ... FOR UPDATE
    user = await User.objects.filter(id=1).for_update().first()
    user.balance -= 100
    await user.save()
```

Or for read-only locks:

```python
async with transaction.atomic():
    # SELECT ... FOR SHARE
    users = await User.objects.filter(status="active").for_share().all()
```

!!! note "SQLite"
    SQLite uses database-level locking. `for_update()` and `for_share()` are no-ops on SQLite.

## Common Patterns

### Transfer Money

```python
from oxyde import F
from oxyde.db import transaction

async def transfer(from_id: int, to_id: int, amount: float):
    async with transaction.atomic():
        # Lock both accounts
        from_acc = await Account.objects.filter(id=from_id).for_update().first()
        to_acc = await Account.objects.filter(id=to_id).for_update().first()

        if from_acc.balance < amount:
            raise ValueError("Insufficient funds")

        await Account.objects.filter(id=from_id).update(
            balance=F("balance") - amount
        )
        await Account.objects.filter(id=to_id).update(
            balance=F("balance") + amount
        )
```

### Create with Related Objects

```python
async def create_user_with_profile(name: str, email: str, bio: str):
    async with transaction.atomic():
        user = await User.objects.create(name=name, email=email)
        await Profile.objects.create(user_id=user.id, bio=bio)
        return user
```

### Bulk Operations

```python
async def import_users(data: list[dict]):
    async with transaction.atomic():
        for item in data:
            await User.objects.create(**item)
        # All or nothing - rolls back if any fails
```

### Conditional Rollback

```python
async def process_order(order_id: int):
    async with transaction.atomic() as tx:
        order = await Order.objects.get(id=order_id)

        if order.status != "pending":
            tx.set_rollback(True)
            return None

        await order.process()
        order.status = "completed"
        await order.save()
        return order
```

### Retry on Conflict

```python
async def increment_with_retry(user_id: int, max_retries: int = 3):
    for attempt in range(max_retries):
        try:
            async with transaction.atomic():
                user = await User.objects.filter(id=user_id).for_update().first()
                user.counter += 1
                await user.save()
                return
        except Exception:
            if attempt == max_retries - 1:
                raise
            await asyncio.sleep(0.1 * (attempt + 1))
```

## Pool-Level Transaction Cleanup

Configure automatic cleanup of leaked transactions:

```python
from oxyde import AsyncDatabase, PoolSettings

db = AsyncDatabase(
    "postgresql://localhost/mydb",
    settings=PoolSettings(
        transaction_timeout=300,           # 5 minutes max
        transaction_cleanup_interval=60,   # Check every minute
    )
)
```

Transactions exceeding the timeout are automatically rolled back.

### Recommended Timeouts

| Workload | Timeout | Cleanup Interval |
|----------|---------|------------------|
| Web API | 5 min | 1 min |
| Background Jobs | 30 min | 5 min |
| Data Analytics | 60 min | 10 min |
| Migrations | 2 hours | 10 min |

## Exceptions

```python
from oxyde.db import transaction, TransactionTimeoutError

try:
    async with transaction.atomic(timeout=10):
        await long_running_operation()
except TransactionTimeoutError:
    print("Transaction exceeded timeout")
```

## Best Practices

### 1. Keep Transactions Short

```python
# GOOD: Short transaction
async with transaction.atomic():
    user = await User.objects.get(id=1)
    user.status = "active"
    await user.save()

# BAD: Long transaction with external calls
async with transaction.atomic():
    user = await User.objects.get(id=1)
    await send_email(user)  # External call inside transaction!
    user.notified = True
    await user.save()
```

### 2. Don't Nest Unnecessarily

```python
# GOOD: Single transaction
async with transaction.atomic():
    await create_user()
    await create_profile()

# UNNECESSARY: Nested transactions
async with transaction.atomic():
    async with transaction.atomic():  # Extra savepoint overhead
        await create_user()
    async with transaction.atomic():
        await create_profile()
```

### 3. Handle Errors Appropriately

```python
# GOOD: Let exceptions propagate
async with transaction.atomic():
    await risky_operation()
    # Exception rolls back transaction

# GOOD: Handle specific cases
async with transaction.atomic():
    try:
        await risky_operation()
    except SpecificError:
        # Handle but still commit other work
        pass
```

### 4. Use Appropriate Isolation

```python
# Use FOR UPDATE when modifying shared data
async with transaction.atomic():
    account = await Account.objects.filter(id=1).for_update().first()
    account.balance -= 100
    await account.save()
```

## Complete Example

```python
import asyncio
from oxyde import Model, Field, db, F
from oxyde.db import transaction, TransactionTimeoutError

class Account(Model):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    name: str
    balance: float = Field(default=0)

async def transfer(from_id: int, to_id: int, amount: float) -> bool:
    """Transfer money between accounts atomically."""
    try:
        async with transaction.atomic(timeout=30):
            # Lock accounts to prevent concurrent modifications
            from_acc = await Account.objects.filter(id=from_id).for_update().first()
            to_acc = await Account.objects.filter(id=to_id).for_update().first()

            if not from_acc or not to_acc:
                return False

            if from_acc.balance < amount:
                return False

            # Update balances atomically
            await Account.objects.filter(id=from_id).update(
                balance=F("balance") - amount
            )
            await Account.objects.filter(id=to_id).update(
                balance=F("balance") + amount
            )

            return True

    except TransactionTimeoutError:
        print("Transfer timed out")
        return False

async def main():
    async with db.connect("sqlite:///bank.db"):
        # Create accounts
        alice = await Account.objects.create(name="Alice", balance=1000)
        bob = await Account.objects.create(name="Bob", balance=500)

        # Transfer money
        success = await transfer(alice.id, bob.id, 200)
        print(f"Transfer successful: {success}")

        # Check balances
        alice = await Account.objects.get(id=alice.id)
        bob = await Account.objects.get(id=bob.id)
        print(f"Alice: ${alice.balance}, Bob: ${bob.balance}")

asyncio.run(main())
```

## Next Steps

- [Migrations](migrations.md) — Database migrations
- [Performance](../advanced/performance.md) — Performance optimization
- [Connections](connections.md) — Connection management

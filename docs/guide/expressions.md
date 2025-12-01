# F-Expressions

F expressions reference database columns for atomic operations executed entirely in the database.

## Why F Expressions?

Without F expressions, you risk race conditions:

```python
# BAD: Race condition
post = await Post.objects.get(id=1)
post.views = post.views + 1  # Read old value
await post.save()            # Another request might have incremented too
```

With F expressions, the operation is atomic:

```python
# GOOD: Atomic
from oxyde import F
await Post.objects.filter(id=1).update(views=F("views") + 1)
```

The SQL generated is `UPDATE posts SET views = views + 1 WHERE id = 1`.

## Basic Usage

```python
from oxyde import F

# Reference a column
F("views")

# Arithmetic
F("views") + 1
F("price") * 0.9
F("balance") - 100
F("total") / F("count")
```

## Supported Operations

| Operation | Example | SQL |
|-----------|---------|-----|
| Addition | `F("x") + 1` | `x + 1` |
| Subtraction | `F("x") - 1` | `x - 1` |
| Multiplication | `F("x") * 2` | `x * 2` |
| Division | `F("x") / 2` | `x / 2` |
| Negation | `-F("x")` | `-x` |

## Column + Column

Combine multiple columns:

```python
# total = price * quantity
await Order.objects.filter(id=1).update(total=F("price") * F("quantity"))

# score = base_score + bonus
await Player.objects.filter(id=1).update(score=F("base_score") + F("bonus"))
```

## Use with update()

The most common use case:

```python
# Increment
await Post.objects.filter(id=1).update(views=F("views") + 1)

# Decrement
await Product.objects.filter(id=1).update(stock=F("stock") - 1)

# Percentage increase
await Product.objects.filter(category="sale").update(
    price=F("price") * 1.1  # 10% increase
)

# Percentage discount
await Product.objects.filter(category="clearance").update(
    price=F("price") * 0.5  # 50% off
)
```

## Use with increment()

Shortcut for common increment pattern:

```python
# These are equivalent:
await Post.objects.filter(id=1).update(views=F("views") + 1)
await Post.objects.filter(id=1).increment("views", by=1)

# Decrement
await Post.objects.filter(id=1).increment("views", by=-1)
```

## Bulk Updates

F expressions work with bulk updates:

```python
# Give everyone a 5% raise
await Employee.objects.update(salary=F("salary") * 1.05)

# Reset all counters
await Stats.objects.update(count=F("count") * 0)  # Set to 0
```

## Complex Expressions

Chain operations:

```python
# (price * quantity) - discount
await Order.objects.filter(id=1).update(
    total=F("price") * F("quantity") - F("discount")
)

# Nested: ((base + bonus) * multiplier)
await Score.objects.filter(id=1).update(
    final=((F("base") + F("bonus")) * F("multiplier"))
)
```

## With Value Constants

Mix F expressions with constants:

```python
# Add flat fee
await Order.objects.filter(id=1).update(total=F("subtotal") + 9.99)

# Apply tax rate
await Order.objects.filter(id=1).update(total=F("subtotal") * 1.08)
```

## Reverse Operations

F expressions support reverse operations:

```python
# 100 - balance (instead of balance - 100)
await Account.objects.filter(id=1).update(remaining=100 - F("spent"))

# 2 * multiplier
await Score.objects.filter(id=1).update(doubled=2 * F("base"))
```

## Examples

### Page View Counter

```python
async def view_post(post_id: int):
    await Post.objects.filter(id=post_id).increment("views")
```

### Inventory Management

```python
async def purchase(product_id: int, quantity: int):
    await Product.objects.filter(id=product_id).update(
        stock=F("stock") - quantity
    )

async def restock(product_id: int, quantity: int):
    await Product.objects.filter(id=product_id).update(
        stock=F("stock") + quantity
    )
```

### Balance Transfer

```python
from oxyde.db import transaction

async def transfer(from_id: int, to_id: int, amount: float):
    async with transaction.atomic():
        await Account.objects.filter(id=from_id).update(
            balance=F("balance") - amount
        )
        await Account.objects.filter(id=to_id).update(
            balance=F("balance") + amount
        )
```

### Pricing Rules

```python
# Apply 20% discount to all sale items
await Product.objects.filter(on_sale=True).update(
    price=F("price") * 0.8
)

# Round up to nearest dollar
await Product.objects.update(
    price=F("price") + 0.99  # Simple approach
)
```

### Score Calculation

```python
# Calculate weighted score
await Score.objects.update(
    total=F("correct") * 10 - F("wrong") * 5
)
```

## Limitations

### No Filtering with F

F expressions are for UPDATE values, not for filtering:

```python
# This won't work
# await User.objects.filter(balance__gt=F("credit_limit")).all()

# Use raw SQL or application logic instead
```

### No String Operations

F expressions only support numeric operations:

```python
# These won't work:
# F("first_name") + " " + F("last_name")  # String concat
# F("name").lower()  # String methods

# Use database functions or compute in application
```

### Read-Only in Select

F expressions are primarily for UPDATE statements. For computed columns in SELECT, use annotations with aggregate functions.

## Next Steps

- [Aggregation](aggregation.md) — Aggregate functions and GROUP BY
- [Queries](queries.md) — Full query reference
- [Transactions](transactions.md) — Atomic operations with transactions

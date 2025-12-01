# Aggregation

Oxyde supports SQL aggregate functions and GROUP BY operations.

## Aggregate Functions

### Direct Aggregates

Execute aggregate queries directly:

```python
# COUNT
count = await User.objects.count()
active_count = await User.objects.filter(status="active").count()

# SUM
total = await Order.objects.sum("amount")

# AVG
average = await User.objects.avg("age")

# MAX / MIN
highest = await Product.objects.max("price")
lowest = await Product.objects.min("price")
```

### Aggregate Classes

For more control, use aggregate classes:

```python
from oxyde import Count, Sum, Avg, Max, Min

# With distinct
unique_count = await User.objects.annotate(
    unique_cities=Count("city", distinct=True)
).all()
```

## Available Aggregates

| Function | Description | Example |
|----------|-------------|---------|
| `Count` | Count rows | `Count("*")`, `Count("id", distinct=True)` |
| `Sum` | Sum values | `Sum("amount")` |
| `Avg` | Average | `Avg("price")` |
| `Max` | Maximum | `Max("created_at")` |
| `Min` | Minimum | `Min("price")` |

## annotate()

Add computed columns to query results:

```python
from oxyde import Count

# Single annotation
results = await User.objects.annotate(
    post_count=Count("id")
).all()

# Multiple annotations
results = await Order.objects.annotate(
    total=Sum("amount"),
    avg_item=Avg("amount"),
).all()
```

## GROUP BY

Group results by one or more columns:

```python
# Posts per author
results = await Post.objects.values("author_id").annotate(
    count=Count("*")
).group_by("author_id").all()

# Result: [{"author_id": 1, "count": 5}, {"author_id": 2, "count": 3}]
```

### Multiple Group Columns

```python
# Sales by year and month
results = await Order.objects.values("year", "month").annotate(
    total=Sum("amount"),
    orders=Count("*")
).group_by("year", "month").all()
```

### With Filtering

```python
# Active users per city
results = await User.objects.filter(
    status="active"
).values("city").annotate(
    count=Count("*")
).group_by("city").all()
```

## HAVING

Filter on aggregate results:

```python
# Authors with more than 5 posts
results = await Post.objects.values("author_id").annotate(
    count=Count("*")
).group_by("author_id").having(count__gt=5).all()
```

!!! note "HAVING vs WHERE"
    - `filter()` → WHERE (filters rows before grouping)
    - `having()` → HAVING (filters groups after aggregation)

### Complex HAVING

```python
# High-value customers (total orders > $1000)
results = await Order.objects.values("customer_id").annotate(
    total=Sum("amount")
).group_by("customer_id").having(total__gte=1000).all()
```

## Ordering Aggregates

Order by aggregate values:

```python
# Top authors by post count
results = await Post.objects.values("author_id").annotate(
    count=Count("*")
).group_by("author_id").order_by("-count").limit(10).all()
```

## Scalar Functions

### Concat

Concatenate string fields:

```python
from oxyde import Concat

results = await User.objects.annotate(
    full_name=Concat("first_name", "last_name", separator=" ")
).all()
```

### Coalesce

Return first non-NULL value:

```python
from oxyde import Coalesce

results = await User.objects.annotate(
    display_name=Coalesce("nickname", "username", "email")
).all()
```

## RawSQL

For unsupported functions, use raw SQL:

```python
from oxyde import RawSQL

results = await User.objects.annotate(
    name_length=RawSQL("LENGTH(name)")
).all()
```

!!! warning "SQL Injection"
    Be careful with user input in RawSQL. Never interpolate user data directly.

## Examples

### Leaderboard

```python
# Top 10 players by score
leaderboard = await Score.objects.values("user_id").annotate(
    total=Sum("points")
).group_by("user_id").order_by("-total").limit(10).all()
```

### Sales Report

```python
# Daily sales totals
from datetime import date

report = await Order.objects.filter(
    created_at__gte=date(2024, 1, 1)
).values("date").annotate(
    orders=Count("*"),
    revenue=Sum("total"),
    avg_order=Avg("total")
).group_by("date").order_by("date").all()
```

### Category Statistics

```python
# Products per category with price stats
stats = await Product.objects.values("category_id").annotate(
    count=Count("*"),
    avg_price=Avg("price"),
    min_price=Min("price"),
    max_price=Max("price")
).group_by("category_id").all()
```

### User Activity

```python
# Most active users (by login count)
active_users = await LoginLog.objects.values("user_id").annotate(
    logins=Count("*")
).group_by("user_id").having(logins__gte=10).order_by("-logins").all()
```

### Time-based Grouping

```python
# Orders by year
yearly = await Order.objects.filter(
    created_at__year=2024
).values("created_at__year", "created_at__month").annotate(
    count=Count("*"),
    total=Sum("amount")
).group_by("created_at__year", "created_at__month").all()
```

## Complete Example

```python
import asyncio
from oxyde import OxydeModel, Field, db, Count, Sum, Avg

class Order(OxydeModel):
    class Meta:
        is_table = True

    id: int | None = Field(default=None, db_pk=True)
    customer_id: int
    amount: float
    status: str

async def main():
    async with db.connect("sqlite:///orders.db"):
        # Total orders and revenue
        total_orders = await Order.objects.count()
        total_revenue = await Order.objects.sum("amount")
        print(f"Orders: {total_orders}, Revenue: ${total_revenue}")

        # Revenue by status
        by_status = await Order.objects.values("status").annotate(
            count=Count("*"),
            total=Sum("amount")
        ).group_by("status").all()

        for row in by_status:
            print(f"{row['status']}: {row['count']} orders, ${row['total']}")

        # Top customers
        top = await Order.objects.values("customer_id").annotate(
            orders=Count("*"),
            spent=Sum("amount")
        ).group_by("customer_id").order_by("-spent").limit(5).all()

        print("\nTop customers:")
        for row in top:
            print(f"Customer {row['customer_id']}: ${row['spent']}")

asyncio.run(main())
```

## Next Steps

- [Relations](relations.md) — Foreign keys and joins
- [Transactions](transactions.md) — Atomic operations
- [Queries](queries.md) — Full query reference

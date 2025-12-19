# Raw Queries

For complex queries not expressible through the ORM, Oxyde supports raw SQL.

!!! warning "SQL Injection"
    Always use parameterized queries. Never interpolate user input directly into SQL strings.

## execute_raw()

The primary way to execute raw SQL:

```python
from oxyde import execute_raw

# Simple SELECT
users = await execute_raw("SELECT * FROM users WHERE age > $1", [18])

# Returns list of dicts
for user in users:
    print(user["name"], user["email"])
```

### Connection Resolution

`execute_raw()` uses the same connection resolution as `Model.objects`:

1. Active transaction (if inside `atomic()`)
2. Named connection (`using="alias"`)
3. Default connection

```python
# Uses default connection
results = await execute_raw("SELECT * FROM users")

# Uses specific connection
results = await execute_raw("SELECT * FROM metrics", using="analytics")

# Inside transaction - automatically uses same transaction
async with transaction.atomic():
    await User.objects.create(name="Alice")
    await execute_raw(
        "INSERT INTO audit_log (user_id, action) VALUES ($1, $2)",
        [1, "created"]
    )
    # Both operations in same transaction
```

## RawSQL in Annotations

Use raw SQL expressions in annotations:

```python
from oxyde import RawSQL

# Custom SQL expression
results = await User.objects.annotate(
    name_length=RawSQL("LENGTH(name)")
).all()

# Database-specific functions
results = await User.objects.annotate(
    created_date=RawSQL("DATE(created_at)")
).all()
```

## Query.sql()

Get the generated SQL without executing:

```python
query = User.objects.filter(age__gte=18).order_by("-created_at").limit(10)

# Get SQL and parameters
sql, params = query.sql()
print(f"SQL: {sql}")
print(f"Params: {params}")

# With specific dialect
sql, params = query.sql(dialect="postgres")
sql, params = query.sql(dialect="sqlite")
sql, params = query.sql(dialect="mysql")
```

## Common Use Cases

### Window Functions

```python
# Window functions (not supported in ORM)
results = await execute_raw("""
    SELECT
        id,
        name,
        salary,
        RANK() OVER (ORDER BY salary DESC) as rank
    FROM employees
    WHERE department_id = $1
""", [department_id])
```

### Database-Specific Features

```python
# PostgreSQL JSONB operators
results = await execute_raw("""
    SELECT * FROM products
    WHERE metadata @> '{"featured": true}'::jsonb
""")

# PostgreSQL full-text search
results = await execute_raw("""
    SELECT * FROM articles
    WHERE to_tsvector('english', content) @@ plainto_tsquery('english', $1)
""", [search_term])

# SQLite JSON functions
results = await execute_raw("""
    SELECT * FROM products
    WHERE json_extract(metadata, '$.featured') = 1
""")
```

### Complex Joins

```python
# Self-join with aliases
results = await execute_raw("""
    SELECT
        e.name as employee,
        m.name as manager
    FROM employees e
    LEFT JOIN employees m ON e.manager_id = m.id
    WHERE e.department_id = $1
""", [department_id])
```

### Recursive CTEs

```python
# Recursive category tree
results = await execute_raw("""
    WITH RECURSIVE category_tree AS (
        SELECT id, name, parent_id, 0 as depth
        FROM categories
        WHERE parent_id IS NULL

        UNION ALL

        SELECT c.id, c.name, c.parent_id, ct.depth + 1
        FROM categories c
        JOIN category_tree ct ON c.parent_id = ct.id
    )
    SELECT * FROM category_tree
    ORDER BY depth, name
""")
```

### Bulk Operations

```python
# UPSERT (PostgreSQL)
sql = """
    INSERT INTO products (sku, name, price)
    VALUES ($1, $2, $3)
    ON CONFLICT (sku)
    DO UPDATE SET
        name = EXCLUDED.name,
        price = EXCLUDED.price
"""

for product in products:
    await execute_raw(sql, [product.sku, product.name, product.price])
```

## Parameter Binding

### PostgreSQL

Uses `$1`, `$2`, etc.:

```python
results = await execute_raw(
    "SELECT * FROM users WHERE age >= $1 AND status = $2",
    [18, "active"]
)
```

### SQLite

Uses `?` placeholders:

```python
results = await execute_raw(
    "SELECT * FROM users WHERE age >= ? AND status = ?",
    [18, "active"]
)
```

### MySQL

Uses `?` placeholders:

```python
results = await execute_raw(
    "SELECT * FROM users WHERE age >= ? AND status = ?",
    [18, "active"]
)
```

## Mixing Raw and ORM

### Filter with Raw SQL

```python
# Get IDs from raw query
results = await execute_raw("""
    SELECT user_id FROM user_scores
    WHERE score > (SELECT AVG(score) FROM user_scores)
""")
user_ids = [r["user_id"] for r in results]

# Use in ORM query
high_scorers = await User.objects.filter(id__in=user_ids).all()
```

### Supplement ORM Queries

```python
# ORM for basic query
users = await User.objects.filter(status="active").all()

# Raw SQL for complex aggregation
user_ids = [u.id for u in users]
stats = await execute_raw("""
    SELECT user_id, COUNT(*) as post_count
    FROM posts
    WHERE user_id = ANY($1)
    GROUP BY user_id
""", [user_ids])
```

## Transaction Support

Raw queries automatically participate in transactions:

```python
from oxyde import atomic, execute_raw

async with atomic():
    # ORM operation
    user = await User.objects.create(name="Alice")

    # Raw SQL in same transaction
    await execute_raw(
        "INSERT INTO audit_log (user_id, action) VALUES ($1, $2)",
        [user.id, "created"]
    )
    # Both commit together or rollback together
```

## explain()

Analyze query performance:

```python
# Using ORM explain
plan = await User.objects.filter(age__gte=18).explain(analyze=True)

# Raw SQL explain
results = await execute_raw("EXPLAIN ANALYZE SELECT * FROM users WHERE age >= 18")
```

## Best Practices

### 1. Prefer ORM When Possible

```python
# Use ORM for standard operations
users = await User.objects.filter(status="active").all()

# Use raw SQL only for unsupported features
results = await execute_raw(
    "SELECT * FROM users WHERE metadata @> $1::jsonb",
    ['{"vip": true}']
)
```

### 2. Parameterize Everything

```python
# GOOD
await execute_raw("SELECT * FROM users WHERE email = $1", [user_email])

# BAD - SQL injection risk!
await execute_raw(f"SELECT * FROM users WHERE email = '{user_email}'")
```

### 3. Document Complex Queries

```python
async def get_user_activity_report(user_id: int):
    """
    Get user activity report with window functions.

    Returns posts with running total of views and rank within user's posts.
    """
    return await execute_raw("""
        SELECT
            id,
            title,
            views,
            SUM(views) OVER (ORDER BY created_at) as running_total,
            RANK() OVER (ORDER BY views DESC) as view_rank
        FROM posts
        WHERE author_id = $1
        ORDER BY created_at DESC
    """, [user_id])
```

### 4. Test Across Databases

```python
def get_date_trunc_sql(field: str, unit: str, dialect: str) -> str:
    """Generate date truncation SQL for different dialects."""
    if dialect == "postgres":
        return f"DATE_TRUNC('{unit}', {field})"
    elif dialect == "sqlite":
        if unit == "day":
            return f"DATE({field})"
        elif unit == "month":
            return f"DATE({field}, 'start of month')"
    elif dialect == "mysql":
        return f"DATE_FORMAT({field}, '%Y-%m-01')"
```

## Limitations

### No Automatic Type Conversion

Raw queries return raw values:

```python
# ORM converts types
user = await User.objects.get(id=1)
print(type(user.created_at))  # datetime

# Raw may return strings (depends on driver and column type)
results = await execute_raw("SELECT created_at FROM users WHERE id = $1", [1])
print(type(results[0]["created_at"]))  # str or datetime
```

### No Model Hydration

Raw queries return dictionaries, not model instances:

```python
# ORM returns models
users = await User.objects.all()
print(type(users[0]))  # User

# Raw returns dicts
results = await execute_raw("SELECT * FROM users")
print(type(results[0]))  # dict
```

To hydrate manually:

```python
rows = await execute_raw("SELECT * FROM users WHERE ...")
users = [User.model_validate(row) for row in rows]
```

## API Reference

```python
async def execute_raw(
    sql: str,
    params: list[Any] | None = None,
    *,
    using: str | None = None,
    client: SupportsExecute | None = None,
) -> list[dict[str, Any]]:
    """
    Execute raw SQL query.

    Args:
        sql: SQL with placeholders ($1/$2 for Postgres, ? for SQLite/MySQL)
        params: Query parameters (use these to prevent SQL injection!)
        using: Connection alias (default: "default")
        client: Explicit client (AsyncDatabase or AsyncTransaction)

    Returns:
        List of dicts for SELECT queries.
        Empty list for INSERT/UPDATE/DELETE without RETURNING.

    Raises:
        RuntimeError: If no connection is available.
        ManagerError: If both 'using' and 'client' are provided.
    """
```

## Next Steps

- [Internals](internals.md) — Rust core architecture
- [Performance](performance.md) — Optimization techniques
- [Queries](../guide/queries.md) — ORM query reference

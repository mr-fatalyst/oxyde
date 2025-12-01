# Raw Queries

For complex queries not expressible through the ORM, Oxyde supports raw SQL.

!!! warning "SQL Injection"
    Always use parameterized queries. Never interpolate user input directly into SQL strings.

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

## Low-Level Execution

For full control, use the database connection directly:

```python
from oxyde import get_connection
import msgpack

async def raw_query(sql: str, params: list = None):
    conn = await get_connection("default")

    # Build raw IR
    ir = {
        "type": "raw",
        "sql": sql,
        "params": params or [],
    }

    # Execute
    result_bytes = await conn.execute(ir)
    return msgpack.unpackb(result_bytes, raw=False)
```

## Common Use Cases

### Complex Aggregations

```python
# Window functions (not supported in ORM)
sql = """
SELECT
    id,
    name,
    salary,
    RANK() OVER (ORDER BY salary DESC) as rank
FROM employees
WHERE department_id = $1
"""

results = await raw_query(sql, [department_id])
```

### Database-Specific Features

```python
# PostgreSQL JSONB operators
sql = """
SELECT * FROM products
WHERE metadata @> '{"featured": true}'::jsonb
"""

# PostgreSQL full-text search
sql = """
SELECT * FROM articles
WHERE to_tsvector('english', content) @@ plainto_tsquery('english', $1)
"""

# SQLite JSON functions
sql = """
SELECT * FROM products
WHERE json_extract(metadata, '$.featured') = 1
"""
```

### Complex Joins

```python
# Self-join with aliases
sql = """
SELECT
    e.name as employee,
    m.name as manager
FROM employees e
LEFT JOIN employees m ON e.manager_id = m.id
WHERE e.department_id = $1
"""
```

### Recursive CTEs

```python
# Recursive category tree
sql = """
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
"""
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

# Bulk UPSERT
for product in products:
    await raw_query(sql, [product.sku, product.name, product.price])
```

## Parameter Binding

### PostgreSQL

Uses `$1`, `$2`, etc.:

```python
sql = "SELECT * FROM users WHERE age >= $1 AND status = $2"
params = [18, "active"]
```

### SQLite

Uses `?` placeholders:

```python
sql = "SELECT * FROM users WHERE age >= ? AND status = ?"
params = [18, "active"]
```

### MySQL

Uses `?` placeholders:

```python
sql = "SELECT * FROM users WHERE age >= ? AND status = ?"
params = [18, "active"]
```

## Mixing Raw and ORM

### Filter with Raw SQL

```python
# Get IDs from raw query
sql = """
SELECT user_id FROM user_scores
WHERE score > (SELECT AVG(score) FROM user_scores)
"""
results = await raw_query(sql)
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
sql = f"""
SELECT user_id, COUNT(*) as post_count
FROM posts
WHERE user_id = ANY($1)
GROUP BY user_id
"""
stats = await raw_query(sql, [user_ids])
```

## Transaction Support

Raw queries participate in transactions:

```python
from oxyde.db import transaction

async with transaction.atomic():
    # ORM operation
    user = await User.objects.create(name="Alice")

    # Raw SQL in same transaction
    sql = "INSERT INTO audit_log (user_id, action) VALUES ($1, $2)"
    await raw_query(sql, [user.id, "created"])
```

## explain()

Analyze raw query performance:

```python
# Using ORM explain
plan = await User.objects.filter(age__gte=18).explain(analyze=True)

# Raw SQL explain
sql = "EXPLAIN ANALYZE SELECT * FROM users WHERE age >= 18"
plan = await raw_query(sql)
```

## Best Practices

### 1. Prefer ORM When Possible

```python
# Use ORM for standard operations
users = await User.objects.filter(status="active").all()

# Use raw SQL only for unsupported features
sql = "SELECT * FROM users WHERE metadata @> '{\"vip\": true}'::jsonb"
```

### 2. Parameterize Everything

```python
# GOOD
sql = "SELECT * FROM users WHERE email = $1"
await raw_query(sql, [user_email])

# BAD - SQL injection risk!
sql = f"SELECT * FROM users WHERE email = '{user_email}'"
```

### 3. Document Complex Queries

```python
async def get_user_activity_report(user_id: int):
    """
    Get user activity report with window functions.

    Returns posts with running total of views and rank within user's posts.
    """
    sql = """
    SELECT
        id,
        title,
        views,
        SUM(views) OVER (ORDER BY created_at) as running_total,
        RANK() OVER (ORDER BY views DESC) as view_rank
    FROM posts
    WHERE author_id = $1
    ORDER BY created_at DESC
    """
    return await raw_query(sql, [user_id])
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

# Raw returns strings (depends on driver)
sql = "SELECT created_at FROM users WHERE id = $1"
result = await raw_query(sql, [1])
print(type(result[0]["created_at"]))  # str or datetime
```

### No Model Hydration

Raw queries return dictionaries, not model instances:

```python
# ORM returns models
users = await User.objects.all()
print(type(users[0]))  # User

# Raw returns dicts
sql = "SELECT * FROM users"
results = await raw_query(sql)
print(type(results[0]))  # dict
```

To hydrate manually:

```python
sql = "SELECT * FROM users WHERE ..."
rows = await raw_query(sql)
users = [User.model_validate(row) for row in rows]
```

## Next Steps

- [Internals](internals.md) — Rust core architecture
- [Performance](performance.md) — Optimization techniques
- [Queries](../guide/queries.md) — ORM query reference

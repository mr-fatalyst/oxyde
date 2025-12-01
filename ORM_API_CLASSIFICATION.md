# ORM API Classification

| Category | Method / Operator | Call Type | SQL Query? | Returns | Purpose / Notes |
|----------|-------------------|-----------|------------|---------|-----------------|
| **Model Instance** | `save()` | `await user.save()` | Yes (INSERT/UPDATE) | `OxydeModel` | Returns self; `update_fields` for partial update |
| | `delete()` | `await user.delete()` | Yes (DELETE) | `int` | Delete current instance, returns count |
| | `refresh()` | `await user.refresh()` | Yes (SELECT) | `OxydeModel` | Reload data from DB, returns self |
| | `pre_save()` | override method | - | - | Hook before save/create; `is_create`, `update_fields` |
| | `post_save()` | override method | - | - | Hook after save/create; `is_create`, `update_fields` |
| | `pre_delete()` | override method | - | - | Hook before delete |
| | `post_delete()` | override method | - | - | Hook after delete |
| **Manager Methods** | `create(**fields)` | `await User.objects.create(name="John")` | Yes (INSERT) | `Model` | Insert + return model |
| | `bulk_create(list[Model])` | `await User.objects.bulk_create([...])` | Yes (INSERT) | `list[Model]` | Bulk INSERT, returns list of models with IDs |
| | `bulk_update(list[Model], fields=[...])` | `await User.objects.bulk_update([...], ["age"])` | Yes (UPDATE) | `int` | Bulk UPDATE |
| | `get(**lookups)` | `await User.objects.get(id=42)` | Yes (SELECT) | `Model` | 0 -> DoesNotExist, >1 -> MultipleObjects |
| | `get_or_none(**lookups)` | `await User.objects.get_or_none(id=42)` | Yes (SELECT) | `Model \| None` | Returns None if not found |
| | `get_or_create(**lookups, defaults={})` | `await User.objects.get_or_create(email="...", defaults={...})` | Yes (SELECT+INSERT) | `(Model, bool)` | ON CONFLICT / atomic get-or-insert |
| | `count()` | `await User.objects.count()` | Yes (COUNT) | `int` | Count all records (for filters: `.filter().count()`) |
| **Builder Methods (Query)** | `filter(**lookups)` | `.filter(is_active=True, age__gte=18)` | - | `Query` | WHERE conditions; chainable |
| | `exclude(**lookups)` | `.exclude(status="banned")` | - | `Query` | WHERE NOT conditions; chainable |
| | `order_by(*fields)` | `.order_by("-created_at", "name")` | - | `Query` | ORDER BY; chainable |
| | `limit(n)` | `.limit(10)` | - | `Query` | LIMIT; chainable |
| | `offset(n)` | `.offset(20)` | - | `Query` | OFFSET; chainable |
| | `prefetch(*relations)` | `.prefetch("posts", "comments")` | - | `Query` | Load reverse FK/M2M (strings); chainable |
| | `join(*relations)` | `.join("author", "category")` | - | `Query` | JOIN for FK (strings); chainable |
| | `distinct(bool)` | `.distinct()` or `.distinct(False)` | - | `Query` | DISTINCT; chainable |
| | `group_by(*fields)` | `.group_by("status", "category")` | - | `Query` | GROUP BY; chainable |
| | `having(**conditions)` | `.having(count__gte=5)` | - | `Query` | HAVING after group_by; chainable |
| | `annotate(**expressions)` | `.annotate(posts_count=Count("posts"))` | - | `Query` | Add computed fields; chainable |
| | `union(qs)` | `.union(other_qs)` | - | `Query` | UNION (removes duplicates); chainable |
| | `union_all(qs)` | `.union_all(other_qs)` | - | `Query` | UNION ALL (keeps duplicates); chainable |
| | `for_update()` | `.for_update()` | - | `Query` | FOR UPDATE lock; no-op on SQLite |
| | `for_share()` | `.for_share()` | - | `Query` | FOR SHARE lock; no-op on SQLite |
| **Terminal Methods (Query)** | `all()` | `await qs.all()` | Yes (SELECT) | `list[Model]` | **Required** to execute selection |
| | `first()` | `await qs.first()` | Yes (SELECT) | `Model \| None` | First result or None |
| | `last()` | `await qs.last()` | Yes (SELECT) | `Model \| None` | Last result (requires order_by) |
| | `count()` | `await qs.count()` | Yes (COUNT) | `int` | Record count for Query |
| | `exists()` | `await qs.exists()` | Yes (EXISTS) | `bool` | Existence check |
| | `delete()` | `await qs.delete()` | Yes (DELETE) | `int` | Bulk-delete by Query conditions |
| | `update(**fields)` | `await qs.update(status="archived")` | Yes (UPDATE) | `int` | Bulk-update by Query conditions |
| | `increment(field, by=1)` | `await qs.increment("views", by=1)` | Yes (UPDATE) | `int` | Atomic increment by Query |
| | `sum(field)` | `await qs.sum("views")` | Yes (SUM) | `int \| float` | Sum of values |
| | `avg(field)` | `await qs.avg("age")` | Yes (AVG) | `float` | Average value |
| | `max(field)` | `await qs.max("price")` | Yes (MAX) | `Any` | Maximum |
| | `min(field)` | `await qs.min("price")` | Yes (MIN) | `Any` | Minimum |
| | `values(*cols)` | `await qs.values("id", "name")` | Yes (SELECT) | `list[dict]` | Projection without models |
| | `values_list(*cols, flat=False)` | `await qs.values_list("id", flat=True)` | Yes (SELECT) | `list[tuple] \| list` | Tuple projection |
| | **Note:** | All terminal methods accept `using="db_alias"` | - | - | For DB selection: `await qs.all(using="replica")` |
| **Introspection** | `sql(dialect="postgres")` | `qs.sql()` | - | `(str, list)` | Raw SQL + params for logging |
| | `query()` | `qs.query()` | - | `dict` | Serialized IR (Intermediate Representation) |
| | `explain(format, analyze)` | `await qs.explain()` or `await qs.explain(analyze=True)` | Yes (EXPLAIN) | `str \| dict` | Query execution plan |
| **Expressions** | `Q` | `Q(age__gte=18) & Q(status="active")` | - | - | Logical `&, \|, ~` for complex conditions |
| | `F` | `F("views") + 1` | - | - | Field references / arithmetic for atomic operations |
| | `Count` | `Count("posts")` | - | - | COUNT aggregate function for annotate |
| | `Sum` | `Sum("amount")` | - | - | SUM aggregate function for annotate |
| | `Avg` | `Avg("age")` | - | - | AVG aggregate function for annotate |
| | `Max` | `Max("price")` | - | - | MAX aggregate function for annotate |
| | `Min` | `Min("price")` | - | - | MIN aggregate function for annotate |
| | `Concat` | `Concat("first_name", " ", "last_name")` | - | - | String concatenation for annotate |
| | `Coalesce` | `Coalesce("nickname", "username")` | - | - | Return first non-NULL for annotate |
| | `RawSQL` | `RawSQL("LOWER(name)")` | - | - | Raw SQL fragment (escape-safe) |
| **Transactions** | `transaction.atomic()` | `async with transaction.atomic(): ...` | Yes (BEGIN/COMMIT) | `AsyncContextManager` | Django-style atomic transactions with nesting support |
| **Reserved Names** | `save, delete, refresh, objects, pk, id` | - | - | - | Forbidden for Pydantic fields |

---

## Usage Examples

### Basic Operations

```python
# Simple query
users = await User.objects.filter(is_active=True, age__gte=18).all()

# With prefetch and ordering
users = await User.objects \
    .prefetch("posts") \
    .filter(is_active=True) \
    .order_by("-created_at") \
    .limit(10) \
    .all()

# Join for FK
posts = await Post.objects.join("author").filter(status="draft").all()

# Get single object
user = await User.objects.get(id=42)
user = await User.objects.get_or_none(email="test@test.com")
```

### Complex Conditions

```python
# Q expressions
users = await User.objects.filter(
    Q(age__gte=18) & (Q(status="active") | Q(status="premium"))
).all()

# Exclusion
users = await User.objects.exclude(status="banned").all()
```

### Create

```python
# Simple create
user = await User.objects.create(name="John", age=25)

# Get or create
user, created = await User.objects.get_or_create(
    email="test@test.com",
    defaults={"name": "John", "age": 25}
)

# Bulk create
users = await User.objects.bulk_create([
    User(name="John", age=25),
    User(name="Jane", age=30),
])
```

### Update

```python
# Instance update (full UPDATE)
user.age = 26
await user.save()

# Partial update via Query
count = await User.objects.filter(is_active=False).update(status="archived")

# Atomic update with F()
count = await User.objects.filter(id=42).update(views=F("views") + 1)

# Atomic increment
count = await Post.objects.filter(id=42).increment("views", by=1)

# Bulk update
count = await User.objects.bulk_update(
    [user1, user2, user3],
    fields=["status", "updated_at"]
)
```

### Delete

```python
# Instance delete
await user.delete()

# Bulk delete via Query
count = await User.objects.filter(is_active=False).delete()
```

### Aggregation and Annotation

```python
# Annotate - add computed fields
users = await User.objects \
    .annotate(posts_count=Count("posts")) \
    .filter(posts_count__gt=5) \
    .all()

# Group by + having
stats = await Post.objects \
    .group_by("status") \
    .annotate(count=Count("id"), avg_views=Avg("views")) \
    .having(count__gte=10) \
    .all()

# Terminal aggregates
total_views = await Post.objects.filter(status="published").sum("views")
avg_age = await User.objects.filter(is_active=True).avg("age")
max_price = await Product.objects.max("price")
```

### Count and Exists

```python
# Count with filters
count = await User.objects.filter(is_active=True).count()

# Count all records
count = await User.objects.count()

# Exists (faster than count for existence check)
has_active = await User.objects.filter(is_active=True).exists()
```

### Values and values_list

```python
# Dictionaries instead of models
users_data = await User.objects.filter(is_active=True).values("id", "name", "email")
# [{"id": 1, "name": "John", "email": "..."}, ...]

# Flat list
user_ids = await User.objects.filter(is_active=True).values_list("id", flat=True)
# [1, 2, 3, 4, ...]

# Tuples
user_pairs = await User.objects.values_list("id", "name")
# [(1, "John"), (2, "Jane"), ...]
```

### Advanced Queries

```python
# Union
active = User.objects.filter(status="active")
premium = User.objects.filter(status="premium")
combined = await active.union(premium).all()

# Distinct
unique_statuses = await Post.objects.distinct().values_list("status", flat=True)

# First / Last
newest = await Post.objects.order_by("-created_at").first()
oldest = await Post.objects.order_by("created_at").first()
```

### Transactions

```python
from oxyde.db import transaction

# Django-style atomic transactions
async with transaction.atomic():
    user = await User.objects.create(name="John")
    await Post.objects.create(author_id=user.id, title="First post")
    # Rollback on exception

# Nested transactions (savepoints)
async with transaction.atomic():
    user = await User.objects.create(name="John")
    async with transaction.atomic():  # Creates savepoint
        await Post.objects.create(author_id=user.id, title="First post")
```

### Introspection

```python
# SQL query
qs = User.objects.filter(age__gte=18).order_by("-created_at")
sql, params = qs.sql()
print(f"SQL: {sql}")
print(f"Params: {params}")

# Query IR
query_dict = qs.query()

# Execution plan
plan = await qs.explain()
print(plan)

# With analyze
plan = await qs.explain(analyze=True)
```

### Multiple Databases

```python
# Use replica for reading
users = await User.objects.filter(is_active=True).all(using="replica")

# Write to master
user = await User.objects.create(name="John", using="master")
```

### Refresh Instance

```python
user = await User.objects.get(id=42)
# ... time passes, data may have changed in DB ...
await user.refresh()  # Reload from DB
```

---

## Architecture Notes

### Query Builder (Lazy Evaluation)

```python
# These methods DON'T execute SQL, they just build Query
qs = User.objects.filter(is_active=True)  # Query object
qs = qs.prefetch("posts")                  # Query object
qs = qs.order_by("-created_at")            # Query object

# SQL executes only on terminal method call
users = await qs.all()  # <-- SELECT executes here
```

### Query Reuse

```python
active_users = User.objects.filter(is_active=True)

# Different operations with same filter
users = await active_users.all()
count = await active_users.count()
await active_users.delete()
```

### Type Safety

```python
# Fields are auto-completed via Pydantic model
users = await User.objects.filter(
    age__gte=18,        # IDE suggests 'age'
    name__icontains=""  # IDE suggests 'name'
).all()
```

### Prefetch vs Join

```python
# join() - for FK (single query with JOIN)
posts = await Post.objects.join("author").all()
# SQL: SELECT * FROM posts JOIN users ON ...

# prefetch() - for reverse FK / M2M (separate query)
users = await User.objects.prefetch("posts").all()
# SQL: SELECT * FROM users;
#      SELECT * FROM posts WHERE author_id IN (...);
```

---

## Lookup Operators

| Lookup | Description | Example |
|--------|-------------|---------|
| `field` | Equality | `age=18` |
| `field__exact` | Exact equality (same as `=`) | `name__exact="John"` |
| `field__iexact` | Case-insensitive equality | `email__iexact="TEST@EXAMPLE.COM"` |
| `field__contains` | Contains (LIKE '%...%') | `name__contains="oh"` |
| `field__icontains` | Case-insensitive contains | `name__icontains="OH"` |
| `field__startswith` | Starts with | `name__startswith="Jo"` |
| `field__istartswith` | Case-insensitive starts with | `name__istartswith="jo"` |
| `field__endswith` | Ends with | `email__endswith="@example.com"` |
| `field__iendswith` | Case-insensitive ends with | `email__iendswith="@EXAMPLE.COM"` |
| `field__gt` | Greater than (>) | `age__gt=18` |
| `field__gte` | Greater than or equal (>=) | `age__gte=18` |
| `field__lt` | Less than (<) | `age__lt=65` |
| `field__lte` | Less than or equal (<=) | `age__lte=65` |
| `field__in` | In list | `status__in=["active", "premium"]` |
| `field__isnull` | NULL check | `deleted_at__isnull=True` |
| `field__range` | Between (BETWEEN) | `age__range=(18, 65)` |
| `field__between` | Alias for range | `age__between=(18, 65)` |
| `field__year` | Year from date | `created_at__year=2024` |
| `field__month` | Month from date | `created_at__month=(2024, 12)` |
| `field__day` | Day from date | `created_at__day=(2024, 12, 25)` |

---

## Implementation Status v0

### Implemented

- Instance: `save()`, `delete()`, `refresh()`
- Lifecycle hooks: `pre_save()`, `post_save()`, `pre_delete()`, `post_delete()`
- Manager: `create()`, `get()`, `get_or_none()`, `get_or_create()`, `count()`
- Manager: `bulk_create()`, `bulk_update()`
- Query builder: `filter()`, `exclude()`, `order_by()`, `limit()`, `offset()`
- Query builder: `join()`, `prefetch()`, `distinct()`, `group_by()`, `having()`
- Query builder: `annotate()`, `union()`, `union_all()`
- Query builder: `for_update()`, `for_share()` (pessimistic locking)
- Terminal: `all()`, `first()`, `last()`, `count()`, `exists()`, `delete()`, `update()`
- Terminal: `increment()`, `sum()`, `avg()`, `max()`, `min()`
- Terminal: `values()`, `values_list()`
- Expressions: `Q`, `F`, `Count`, `Sum`, `Avg`, `Max`, `Min`, `Concat`, `Coalesce`, `RawSQL`
- Introspection: `sql()`, `query()`, `explain()`
- Transactions: `atomic()` (with nested savepoints)
- Multi-db: `using` parameter in terminal methods

### Planned for future

- Lookups: `hour`, `minute`, `second`, `regex`, `iregex`
- `distinct(*fields)` - PostgreSQL DISTINCT ON

---

## Best Practices

### DO:

```python
# Reuse Query for multiple operations
active = User.objects.filter(is_active=True)
count = await active.count()
users = await active.all()

# Use bulk operations for performance
await User.objects.bulk_create([user1, user2, user3])

# Use prefetch to avoid N+1
users = await User.objects.prefetch("posts").all()

# Use F() for atomic updates
await Post.objects.filter(id=42).update(views=F("views") + 1)

# Check existence with exists(), not count()
if await User.objects.filter(email=email).exists():
    ...
```

### DON'T:

```python
# Don't make N+1 queries
users = await User.objects.all()
for user in users:
    posts = await Post.objects.filter(author_id=user.id).all()  # BAD

# Use prefetch instead:
users = await User.objects.prefetch("posts").all()  # GOOD

# Don't save() in a loop for bulk updates
for user in users:
    user.status = "archived"
    await user.save()  # BAD

# Use bulk_update or update():
await User.objects.filter(id__in=ids).update(status="archived")  # GOOD

# Don't forget .all() for Query
users = await User.objects.filter(is_active=True)  # BAD - Returns Query
users = await User.objects.filter(is_active=True).all()  # GOOD
```

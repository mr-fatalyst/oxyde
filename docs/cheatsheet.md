# API Cheatsheet

Complete API reference.

<style>
.api-table {
  border-collapse: collapse;
  width: 100%;
}
.api-table th, .api-table td {
  border: 1px solid var(--md-typeset-table-color, #ccc);
  padding: 0.5em 1em;
}
.api-table th[colspan="4"] {
  text-align: center !important;
  background: var(--md-default-bg-color--light, #f5f5f5);
  border-top: 2px solid var(--md-typeset-table-color, #ccc) !important;
}
.api-table thead th {
  background: var(--md-default-bg-color--lighter, #fafafa);
}
</style>

<div markdown>
<table class="api-table">
<thead>
<tr><th>Method</th><th>Example</th><th>Returns</th><th>Notes</th></tr>
</thead>
<tbody>
<tr><th colspan="4">Instance Methods</th></tr>
<tr><td><code>save()</code></td><td><code>await user.save()</code></td><td><code>Self</code></td><td><code>update_fields</code> for partial</td></tr>
<tr><td><code>delete()</code></td><td><code>await user.delete()</code></td><td><code>int</code></td><td>Delete instance</td></tr>
<tr><td><code>refresh()</code></td><td><code>await user.refresh()</code></td><td><code>Self</code></td><td>Reload from DB</td></tr>
<tr><td><code>pre_save()</code></td><td>override</td><td>—</td><td>Hook; <code>is_create</code>, <code>update_fields</code></td></tr>
<tr><td><code>post_save()</code></td><td>override</td><td>—</td><td>Hook; <code>is_create</code>, <code>update_fields</code></td></tr>
<tr><td><code>pre_delete()</code></td><td>override</td><td>—</td><td>Hook before delete</td></tr>
<tr><td><code>post_delete()</code></td><td>override</td><td>—</td><td>Hook after delete</td></tr>
<tr><th colspan="4">Manager Methods</th></tr>
<tr><td><code>create()</code></td><td><code>User.objects.create(name="John")</code></td><td><code>Model</code></td><td>Insert + return</td></tr>
<tr><td><code>bulk_create()</code></td><td><code>User.objects.bulk_create([...])</code></td><td><code>list[Model]</code></td><td>Bulk INSERT</td></tr>
<tr><td><code>bulk_update()</code></td><td><code>User.objects.bulk_update([...], ["age"])</code></td><td><code>int</code></td><td>Bulk UPDATE</td></tr>
<tr><td><code>get()</code></td><td><code>User.objects.get(id=42)</code></td><td><code>Model</code></td><td>Raises if 0 or &gt;1</td></tr>
<tr><td><code>get_or_none()</code></td><td><code>User.objects.get_or_none(id=42)</code></td><td><code>Model | None</code></td><td>None if not found</td></tr>
<tr><td><code>get_or_create()</code></td><td><code>User.objects.get_or_create(email="...")</code></td><td><code>(Model, bool)</code></td><td>Atomic</td></tr>
<tr><td><code>count()</code></td><td><code>User.objects.count()</code></td><td><code>int</code></td><td>Count all</td></tr>
<tr><th colspan="4">Query Builder</th></tr>
<tr><td><code>filter()</code></td><td><code>.filter(is_active=True, age__gte=18)</code></td><td><code>Query</code></td><td>WHERE conditions</td></tr>
<tr><td><code>exclude()</code></td><td><code>.exclude(status="banned")</code></td><td><code>Query</code></td><td>WHERE NOT</td></tr>
<tr><td><code>order_by()</code></td><td><code>.order_by("-created_at")</code></td><td><code>Query</code></td><td>ORDER BY</td></tr>
<tr><td><code>limit()</code></td><td><code>.limit(10)</code></td><td><code>Query</code></td><td>LIMIT</td></tr>
<tr><td><code>offset()</code></td><td><code>.offset(20)</code></td><td><code>Query</code></td><td>OFFSET</td></tr>
<tr><td><code>prefetch()</code></td><td><code>.prefetch("posts")</code></td><td><code>Query</code></td><td>Load reverse FK/M2M</td></tr>
<tr><td><code>join()</code></td><td><code>.join("author")</code></td><td><code>Query</code></td><td>FK JOIN</td></tr>
<tr><td><code>distinct()</code></td><td><code>.distinct()</code></td><td><code>Query</code></td><td>DISTINCT</td></tr>
<tr><td><code>group_by()</code></td><td><code>.group_by("status")</code></td><td><code>Query</code></td><td>GROUP BY</td></tr>
<tr><td><code>having()</code></td><td><code>.having(count__gte=5)</code></td><td><code>Query</code></td><td>HAVING</td></tr>
<tr><td><code>annotate()</code></td><td><code>.annotate(n=Count("posts"))</code></td><td><code>Query</code></td><td>Computed fields</td></tr>
<tr><td><code>union()</code></td><td><code>.union(other_qs)</code></td><td><code>Query</code></td><td>UNION</td></tr>
<tr><td><code>for_update()</code></td><td><code>.for_update()</code></td><td><code>Query</code></td><td>Row lock</td></tr>
<tr><td><code>values()</code></td><td><code>.values("id", "name")</code></td><td><code>Query</code></td><td>Result as dicts</td></tr>
<tr><td><code>values_list()</code></td><td><code>.values_list("id", flat=True)</code></td><td><code>Query</code></td><td>Result as tuples/list</td></tr>
<tr><th colspan="4">Terminal Methods</th></tr>
<tr><td><code>all()</code></td><td><code>await qs.all()</code></td><td><code>list[Model]</code></td><td>Execute SELECT</td></tr>
<tr><td><code>first()</code></td><td><code>await qs.first()</code></td><td><code>Model | None</code></td><td>First row</td></tr>
<tr><td><code>last()</code></td><td><code>await qs.last()</code></td><td><code>Model | None</code></td><td>Last row</td></tr>
<tr><td><code>count()</code></td><td><code>await qs.count()</code></td><td><code>int</code></td><td>COUNT(*)</td></tr>
<tr><td><code>exists()</code></td><td><code>await qs.exists()</code></td><td><code>bool</code></td><td>EXISTS check</td></tr>
<tr><td><code>delete()</code></td><td><code>await qs.delete()</code></td><td><code>int</code></td><td>Bulk DELETE</td></tr>
<tr><td><code>update()</code></td><td><code>await qs.update(status="x")</code></td><td><code>int</code></td><td>Bulk UPDATE</td></tr>
<tr><td><code>increment()</code></td><td><code>await qs.increment("views", by=1)</code></td><td><code>int</code></td><td>Atomic increment</td></tr>
<tr><td><code>sum()</code></td><td><code>await qs.sum("views")</code></td><td><code>number</code></td><td>SUM</td></tr>
<tr><td><code>avg()</code></td><td><code>await qs.avg("age")</code></td><td><code>float</code></td><td>AVG</td></tr>
<tr><td><code>max()</code></td><td><code>await qs.max("price")</code></td><td><code>Any</code></td><td>MAX</td></tr>
<tr><td><code>min()</code></td><td><code>await qs.min("price")</code></td><td><code>Any</code></td><td>MIN</td></tr>
<tr><th colspan="4">Debug & Introspection</th></tr>
<tr><td><code>sql()</code></td><td><code>qs.sql()</code></td><td><code>(str, list)</code></td><td>SQL + params</td></tr>
<tr><td><code>query()</code></td><td><code>qs.query()</code></td><td><code>dict</code></td><td>Query IR</td></tr>
<tr><td><code>explain()</code></td><td><code>await qs.explain(analyze=True)</code></td><td><code>str</code></td><td>Query plan</td></tr>
<tr><th colspan="4">Expressions</th></tr>
<tr><td><code>Q</code></td><td><code>Q(age__gte=18) & Q(status="active")</code></td><td>—</td><td>AND/OR/NOT</td></tr>
<tr><td><code>F</code></td><td><code>F("views") + 1</code></td><td>—</td><td>Field reference</td></tr>
<tr><td><code>Count</code></td><td><code>Count("posts")</code></td><td>—</td><td>Aggregate</td></tr>
<tr><td><code>Sum/Avg/Max/Min</code></td><td><code>Sum("amount")</code></td><td>—</td><td>Aggregates</td></tr>
<tr><td><code>Concat</code></td><td><code>Concat("first", "last")</code></td><td>—</td><td>String concat</td></tr>
<tr><td><code>Coalesce</code></td><td><code>Coalesce("nick", "name")</code></td><td>—</td><td>First non-NULL</td></tr>
<tr><td><code>RawSQL</code></td><td><code>RawSQL("LOWER(name)")</code></td><td>—</td><td>Raw SQL</td></tr>
<tr><th colspan="4">Transactions</th></tr>
<tr><td><code>transaction.atomic()</code></td><td><code>async with transaction.atomic(): ...</code></td><td>—</td><td>Nested savepoints</td></tr>
</tbody>
</table>
</div>

---

## Examples

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

### Aggregation

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

### Count & Exists

```python
# Count with filters
count = await User.objects.filter(is_active=True).count()

# Count all
count = await User.objects.count()

# Exists (faster than count for existence check)
has_active = await User.objects.filter(is_active=True).exists()
```

### Values & Values List

```python
# Dictionaries instead of models (values is a builder, requires .all())
users_data = await User.objects.filter(is_active=True).values("id", "name", "email").all()
# [{"id": 1, "name": "John", "email": "..."}, ...]

# Flat list
user_ids = await User.objects.filter(is_active=True).values_list("id", flat=True).all()
# [1, 2, 3, 4, ...]

# Tuples
user_pairs = await User.objects.values_list("id", "name").all()
# [(1, "John"), (2, "Jane"), ...]
```

### Advanced Queries

```python
# Union
active = User.objects.filter(status="active")
premium = User.objects.filter(status="premium")
combined = await active.union(premium).all()

# Distinct
unique_statuses = await Post.objects.distinct().values_list("status", flat=True).all()

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
# Get SQL query
qs = User.objects.filter(age__gte=18).order_by("-created_at")
sql, params = qs.sql()
print(f"SQL: {sql}")
print(f"Params: {params}")

# Get query IR (Intermediate Representation)
ir = qs.query()
print(ir)

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

## Best Practices

### DO

```python
# Reuse Query for multiple operations
active = User.objects.filter(is_active=True)
count = await active.count()
users = await active.all()

# Use bulk operations for performance
await User.objects.bulk_create([user1, user2, user3])

# Use prefetch to load related objects
users = await User.objects.prefetch("posts").all()

# Use F() for atomic updates
await Post.objects.filter(id=42).update(views=F("views") + 1)

# Check existence with exists(), not count()
if await User.objects.filter(email=email).exists():
    ...
```

### DON'T

```python
# Don't query in a loop
users = await User.objects.all()
for user in users:
    posts = await Post.objects.filter(author_id=user.id).all()  # ❌ Multiple queries

# Use prefetch instead:
users = await User.objects.prefetch("posts").all()  # ✅ Two queries total

# Don't save() in a loop for bulk updates
for user in users:
    user.status = "archived"
    await user.save()  # ❌

# Use bulk_update or update():
await User.objects.filter(id__in=ids).update(status="archived")  # ✅

# Don't forget .all() for Query
users = await User.objects.filter(is_active=True)  # ❌ Returns Query
users = await User.objects.filter(is_active=True).all()  # ✅
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
users = await qs.all()  # ← SELECT executes here
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

## Next Steps

- [Filtering](guide/filtering.md) — Filter conditions and lookups
- [Models](guide/models.md) — Model definition
- [Queries](guide/queries.md) — Query API

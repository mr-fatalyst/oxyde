# Queries

Oxyde uses a Django-style QuerySet API for database queries.

## Query Builder Pattern

Queries are built by chaining methods. The query executes when you call a terminal method like `all()` or `get()`.

```python
# Build query (not executed yet)
query = User.objects.filter(status="active").order_by("-created_at").limit(10)

# Execute query
users = await query.all()
```

## The Manager: Model.objects

Every model has an `objects` manager:

```python
class User(OxydeModel):
    # ... fields ...

    class Meta:
        is_table = True

# Access via Model.objects
users = await User.objects.all()
```

## Query Methods

### Retrieving Objects

#### all()

Get all records:

```python
users = await User.objects.all()
```

With filters:

```python
active_users = await User.objects.filter(status="active").all()
```

#### get()

Get exactly one record (raises exception if not found or multiple found):

```python
from oxyde.exceptions import NotFoundError, MultipleObjectsReturned

try:
    user = await User.objects.get(id=1)
except NotFoundError:
    print("User not found")
except MultipleObjectsReturned:
    print("Multiple users found")
```

#### get_or_none()

Get one record or None:

```python
user = await User.objects.get_or_none(email="alice@example.com")
if user:
    print(f"Found: {user.name}")
```

#### first() / last()

Get first or last record by primary key:

```python
first_user = await User.objects.first()
last_user = await User.objects.last()
```

With filters (use limit):

```python
users = await User.objects.filter(status="active").order_by("-created_at").limit(1).all()
newest_active = users[0] if users else None
```

### Filtering

#### filter()

Add WHERE conditions (AND):

```python
# Simple equality
users = await User.objects.filter(status="active").all()

# Multiple conditions (AND)
users = await User.objects.filter(status="active", age__gte=18).all()

# Chained filters (also AND)
users = await User.objects.filter(status="active").filter(age__gte=18).all()
```

#### exclude()

Exclude matching records:

```python
# NOT status = 'banned'
users = await User.objects.exclude(status="banned").all()

# Combine with filter
users = await User.objects.filter(age__gte=18).exclude(role="bot").all()
```

See [Filtering](filtering.md) for complete lookup reference.

### Ordering

#### order_by()

Sort results:

```python
# Ascending
users = await User.objects.order_by("name").all()

# Descending (prefix with -)
users = await User.objects.order_by("-created_at").all()

# Multiple columns
users = await User.objects.order_by("status", "-created_at").all()
```

### Pagination

#### limit() / offset()

```python
# First 10 records
users = await User.objects.limit(10).all()

# Skip 20, take 10 (pagination)
users = await User.objects.offset(20).limit(10).all()
```

### Selecting Fields

#### values()

Return dictionaries instead of model instances:

```python
# Select specific fields
users = await User.objects.values("id", "email").all()
# [{"id": 1, "email": "alice@example.com"}, ...]
```

#### values_list()

Return tuples:

```python
# As tuples
users = await User.objects.values_list("id", "email").all()
# [(1, "alice@example.com"), ...]

# Flat list (single field only)
ids = await User.objects.values_list("id", flat=True).all()
# [1, 2, 3, ...]
```

#### distinct()

Remove duplicates:

```python
cities = await User.objects.values("city").distinct().all()
```

### Aggregation

#### count()

```python
count = await User.objects.count()
count = await User.objects.filter(status="active").count()
```

#### sum() / avg() / max() / min()

```python
total = await Order.objects.sum("amount")
average = await User.objects.avg("age")
highest = await Product.objects.max("price")
lowest = await Product.objects.min("price")
```

See [Aggregation](aggregation.md) for GROUP BY and HAVING.

### Existence Check

#### exists()

```python
has_admins = await User.objects.filter(role="admin").exists()
if has_admins:
    print("At least one admin exists")
```

### Creating Records

#### create()

```python
user = await User.objects.create(
    name="Alice",
    email="alice@example.com",
    age=30
)
print(user.id)  # Auto-generated
```

#### bulk_create()

Insert multiple records efficiently:

```python
users = [
    User(name="Alice", email="alice@example.com"),
    User(name="Bob", email="bob@example.com"),
    User(name="Carol", email="carol@example.com"),
]
created = await User.objects.bulk_create(users)
```

With batching:

```python
# Insert in batches of 100
created = await User.objects.bulk_create(users, batch_size=100)
```

#### get_or_create()

Get existing or create new:

```python
user, created = await User.objects.get_or_create(
    email="alice@example.com",
    defaults={"name": "Alice", "age": 30}
)
if created:
    print("Created new user")
else:
    print("Found existing user")
```

### Updating Records

#### update()

Bulk update matching records:

```python
# Returns count of affected rows
count = await User.objects.filter(status="pending").update(status="active")
```

With F expressions:

```python
from oxyde import F

# Atomic increment
await Post.objects.filter(id=1).update(views=F("views") + 1)
```

#### bulk_update()

Update multiple model instances:

```python
users = await User.objects.filter(status="pending").all()
for user in users:
    user.status = "active"

count = await User.objects.bulk_update(users, ["status"])
```

#### increment()

Atomic field increment:

```python
await Post.objects.filter(id=1).increment("views", by=1)
await Product.objects.filter(id=1).increment("stock", by=-1)  # Decrement
```

### Deleting Records

#### delete()

Bulk delete matching records:

```python
count = await User.objects.filter(status="deleted").delete()
```

### Joins

#### join()

Eager load related models:

```python
# Load author with each post
posts = await Post.objects.join("author").all()
for post in posts:
    print(f"{post.title} by {post.author.name}")
```

#### prefetch()

Load reverse relations:

```python
# Load posts for each author
authors = await Author.objects.prefetch("posts").all()
for author in authors:
    print(f"{author.name} has {len(author.posts)} posts")
```

See [Relations](relations.md) for more.

### Locking

#### for_update()

Lock rows for update (PostgreSQL/MySQL):

```python
from oxyde.db import transaction

async with transaction.atomic():
    user = await User.objects.filter(id=1).for_update().first()
    user.balance -= 100
    await user.save()
```

#### for_share()

Lock rows for reading:

```python
async with transaction.atomic():
    users = await User.objects.filter(status="active").for_share().all()
```

### Union

Combine query results:

```python
admins = User.objects.filter(role="admin")
moderators = User.objects.filter(role="moderator")

# UNION (distinct)
staff = await admins.union(moderators).all()

# UNION ALL (keep duplicates)
all_staff = await admins.union_all(moderators).all()
```

### Debugging

#### sql()

Get generated SQL:

```python
query = User.objects.filter(age__gte=18).limit(10)
sql, params = query.sql()
print(f"SQL: {sql}")
print(f"Params: {params}")
```

With specific dialect:

```python
sql, params = query.sql(dialect="postgres")
```

#### explain()

Get query plan:

```python
plan = await User.objects.filter(age__gte=18).explain()
print(plan)

# With actual execution times
plan = await User.objects.filter(age__gte=18).explain(analyze=True)
```

## Query Immutability

Queries are immutable. Each method returns a new query:

```python
base = User.objects.filter(status="active")
admins = base.filter(role="admin")    # base is unchanged
users = base.filter(role="user")      # base is unchanged

# base, admins, and users are different queries
```

## Specifying Database

Use `using` parameter to specify which database:

```python
# Default database
users = await User.objects.all()

# Specific database
users = await User.objects.all(using="analytics")
```

## Next Steps

- [Filtering](filtering.md) — Complete lookup reference
- [Expressions](expressions.md) — F expressions for database operations
- [Aggregation](aggregation.md) — GROUP BY and aggregate functions

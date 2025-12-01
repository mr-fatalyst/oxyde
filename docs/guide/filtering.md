# Filtering

Oxyde supports Django-style field lookups for filtering queries.

## Basic Filtering

```python
# Exact match (default)
users = await User.objects.filter(status="active").all()

# Multiple conditions (AND)
users = await User.objects.filter(status="active", age__gte=18).all()
```

## Lookup Syntax

Lookups use double underscore notation: `field__lookup=value`

```python
filter(age__gte=18)      # age >= 18
filter(name__contains="john")  # name LIKE '%john%'
```

If no lookup is specified, `exact` is used:

```python
filter(status="active")        # status = 'active'
filter(status__exact="active") # same
```

## Comparison Lookups

| Lookup | SQL | Example |
|--------|-----|---------|
| `exact` | `=` | `filter(age=18)` |
| `gt` | `>` | `filter(age__gt=18)` |
| `gte` | `>=` | `filter(age__gte=18)` |
| `lt` | `<` | `filter(age__lt=65)` |
| `lte` | `<=` | `filter(age__lte=65)` |

```python
# Age between 18 and 65 (inclusive)
users = await User.objects.filter(age__gte=18, age__lte=65).all()
```

## Range Lookups

### between

```python
# BETWEEN 18 AND 65
users = await User.objects.filter(age__between=[18, 65]).all()
```

### range

Alias for `between`:

```python
users = await User.objects.filter(age__range=[18, 65]).all()
```

## String Lookups

| Lookup | SQL | Case Sensitive |
|--------|-----|----------------|
| `contains` | `LIKE '%...%'` | Yes |
| `icontains` | `ILIKE '%...%'` | No |
| `startswith` | `LIKE '...%'` | Yes |
| `istartswith` | `ILIKE '...%'` | No |
| `endswith` | `LIKE '%...'` | Yes |
| `iendswith` | `ILIKE '%...'` | No |
| `iexact` | `LOWER(...) = LOWER(...)` | No |

```python
# Contains (case-sensitive)
users = await User.objects.filter(name__contains="john").all()

# Contains (case-insensitive)
users = await User.objects.filter(name__icontains="john").all()

# Starts with
users = await User.objects.filter(email__startswith="admin@").all()

# Ends with
users = await User.objects.filter(email__endswith="@example.com").all()

# Exact (case-insensitive)
users = await User.objects.filter(email__iexact="JOHN@EXAMPLE.COM").all()
```

## NULL Checks

### isnull

```python
# IS NULL
users = await User.objects.filter(deleted_at__isnull=True).all()

# IS NOT NULL
users = await User.objects.filter(deleted_at__isnull=False).all()
```

## IN Lookup

```python
# IN clause
users = await User.objects.filter(status__in=["active", "pending"]).all()

# With subquery (manual)
admin_ids = await User.objects.filter(role="admin").values_list("id", flat=True).all()
posts = await Post.objects.filter(author_id__in=admin_ids).all()
```

## Date Lookups

### year / month / day

Extract date parts:

```python
# Posts from 2024
posts = await Post.objects.filter(created_at__year=2024).all()

# Posts from December 2024
posts = await Post.objects.filter(created_at__month=(2024, 12)).all()

# Posts from December 25, 2024
posts = await Post.objects.filter(created_at__day=(2024, 12, 25)).all()
```

## Q Expressions

For complex boolean logic (OR, NOT):

```python
from oxyde import Q

# OR
users = await User.objects.filter(
    Q(role="admin") | Q(role="moderator")
).all()

# NOT
users = await User.objects.filter(~Q(status="banned")).all()

# Complex
users = await User.objects.filter(
    Q(age__gte=18) & (Q(status="active") | Q(status="premium"))
).all()
```

### Combining Q Objects

| Operator | Meaning |
|----------|---------|
| `&` | AND |
| `\|` | OR |
| `~` | NOT |

```python
# (age >= 18 AND status = 'active') OR role = 'admin'
users = await User.objects.filter(
    (Q(age__gte=18) & Q(status="active")) | Q(role="admin")
).all()

# NOT (status = 'banned' OR status = 'suspended')
users = await User.objects.filter(
    ~(Q(status="banned") | Q(status="suspended"))
).all()
```

### Mixing Q and kwargs

```python
# Q expressions with keyword arguments
users = await User.objects.filter(
    Q(role="admin") | Q(role="moderator"),
    status="active"  # AND with the Q expression
).all()
```

## exclude()

Negate conditions:

```python
# NOT status = 'banned'
users = await User.objects.exclude(status="banned").all()

# Equivalent to
users = await User.objects.filter(~Q(status="banned")).all()
```

Chain with filter:

```python
# status = 'active' AND NOT role = 'bot'
users = await User.objects.filter(status="active").exclude(role="bot").all()
```

## Lookup Reference by Type

### String Fields

- `exact`, `iexact`
- `contains`, `icontains`
- `startswith`, `istartswith`
- `endswith`, `iendswith`
- `in`, `isnull`

### Numeric Fields (int, float, Decimal)

- `exact`
- `gt`, `gte`, `lt`, `lte`
- `between`, `range`
- `in`, `isnull`

### DateTime/Date Fields

- `exact`
- `gt`, `gte`, `lt`, `lte`
- `between`, `range`
- `year`, `month`, `day`
- `in`, `isnull`

### Boolean Fields

- `exact`
- `in`, `isnull`

## Common Patterns

### Pagination

```python
page = 2
per_page = 20
users = await User.objects.filter(
    status="active"
).order_by("-created_at").offset((page - 1) * per_page).limit(per_page).all()
```

### Search

```python
query = "john"
users = await User.objects.filter(
    Q(name__icontains=query) | Q(email__icontains=query)
).all()
```

### Date Range

```python
from datetime import datetime, timedelta

now = datetime.utcnow()
last_week = now - timedelta(days=7)

recent = await Post.objects.filter(
    created_at__gte=last_week,
    created_at__lt=now
).all()
```

### Active/Soft Delete

```python
# Active records only
active = await User.objects.filter(deleted_at__isnull=True).all()

# Include deleted
all_users = await User.objects.all()
```

## Known Limitations

### No Related Field Lookups

Oxyde does not support Django-style `filter(author__age__gte=18)`. Use subqueries instead:

```python
# Instead of: Post.objects.filter(author__age__gte=18)

adult_ids = await Author.objects.filter(age__gte=18).values_list("id", flat=True).all()
posts = await Post.objects.filter(author_id__in=adult_ids).all()
```

### Ambiguous Columns with JOIN

When using `join()`, filter on FK column to avoid ambiguity:

```python
# May be ambiguous if both tables have 'id'
posts = await Post.objects.join("author").filter(id__in=[1,2,3]).all()

# Better: filter before join or use FK column
posts = await Post.objects.filter(id__in=[1,2,3]).join("author").all()
```

## Next Steps

- [Expressions](expressions.md) — F expressions for database operations
- [Aggregation](aggregation.md) — GROUP BY and aggregate functions
- [Queries](queries.md) — Full query reference

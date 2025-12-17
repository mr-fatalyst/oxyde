# Exceptions API Reference

Complete reference for Oxyde exception hierarchy.

## Overview

All Oxyde exceptions inherit from `OxydeError`, allowing catch-all handling:

```python
from oxyde import OxydeError

try:
    user = await User.objects.get(id=999)
except OxydeError as e:
    print(f"ORM error: {e}")
```

## Exception Hierarchy

```
OxydeError (base)
├── FieldError                - Invalid field definition or access
├── FieldLookupError          - Unknown lookup operator
│   └── FieldLookupValueError - Invalid value for lookup
└── ManagerError              - Query execution errors
    ├── NotFoundError         - get() returned no rows
    ├── MultipleObjectsReturned - get() returned multiple rows
    └── IntegrityError        - Constraint violation
```

## Base Exception

### OxydeError

```python
class OxydeError(Exception):
    """Base exception for all Oxyde-related errors."""
```

Catch this to handle any Oxyde error:

```python
try:
    await some_orm_operation()
except OxydeError as e:
    logger.error(f"Database operation failed: {e}")
    raise HTTPException(500, "Internal server error")
```

## Field Errors

### FieldError

```python
class FieldError(OxydeError):
    """Raised when a model field is invalid or missing."""
```

**When raised:**

- Accessing non-existent field in filter
- Invalid field in `update_fields`
- Field metadata parsing errors

```python
try:
    await User.objects.filter(nonexistent_field="value").all()
except FieldError as e:
    print(f"Field error: {e}")
    # "User has no field 'nonexistent_field'"
```

```python
try:
    user = await User.objects.get(id=1)
    await user.save(update_fields=["invalid_field"])
except FieldError as e:
    print(f"Invalid field: {e}")
```

## Lookup Errors

### FieldLookupError

```python
class FieldLookupError(OxydeError):
    """Raised when an unsupported field lookup is requested."""
```

**When raised:**

- Using unknown lookup suffix (e.g., `__xyz`)

```python
try:
    await User.objects.filter(name__unknown="value").all()
except FieldLookupError as e:
    print(f"Lookup error: {e}")
    # "Unsupported lookup 'unknown' for field 'name'"
```

### FieldLookupValueError

```python
class FieldLookupValueError(FieldLookupError):
    """Raised when the lookup value is not compatible with the operator."""
```

**When raised:**

- `__in` with non-iterable value
- `__in` with string value (likely mistake)
- `__gte`/`__lte` with None
- `__contains` with non-string
- `__between` with wrong tuple size

```python
try:
    await User.objects.filter(id__in="not_a_list").all()
except FieldLookupValueError as e:
    print(f"Value error: {e}")
    # "Lookup 'in' does not accept string values; use a sequence"
```

```python
try:
    await User.objects.filter(age__gte=None).all()
except FieldLookupValueError as e:
    print(f"Value error: {e}")
    # "Lookup 'gte' requires a non-null value"
```

```python
try:
    await User.objects.filter(age__between=(18,)).all()
except FieldLookupValueError as e:
    print(f"Value error: {e}")
    # "Lookup 'between' requires a tuple/list of two values"
```

## Manager Errors

### ManagerError

```python
class ManagerError(OxydeError):
    """Raised for issues inside the ORM manager layer."""
```

**When raised:**

- General query execution failures
- Invalid query construction
- Missing required parameters

```python
try:
    await User.objects.create()  # No data provided
except ManagerError as e:
    print(f"Manager error: {e}")
    # "create() requires an instance or field values"
```

```python
try:
    user = User(name="Test")  # No PK
    await user.delete()
except ManagerError as e:
    print(f"Manager error: {e}")
    # "delete() requires the instance to have a primary key value"
```

### NotFoundError

```python
class NotFoundError(ManagerError):
    """Raised when a query expecting a single row finds none."""
```

**When raised:**

- `get()` finds no matching rows
- `save()` update finds no matching row

```python
try:
    user = await User.objects.get(id=999999)
except NotFoundError as e:
    print(f"Not found: {e}")
    # "User matching query not found"
```

**Common pattern:**

```python
# Option 1: Use get_or_none
user = await User.objects.get_or_none(id=user_id)
if user is None:
    return {"error": "User not found"}

# Option 2: Catch exception
try:
    user = await User.objects.get(id=user_id)
except NotFoundError:
    raise HTTPException(404, "User not found")
```

### MultipleObjectsReturned

```python
class MultipleObjectsReturned(ManagerError):
    """Raised when a query expecting a single row finds more than one."""
```

**When raised:**

- `get()` finds multiple matching rows

```python
try:
    user = await User.objects.get(status="active")  # Many active users!
except MultipleObjectsReturned as e:
    print(f"Multiple found: {e}")
    # "Query for User returned multiple objects"
```

**Prevention:**

```python
# Use unique fields for get()
user = await User.objects.get(email="alice@example.com")

# Or use first() for non-unique queries
user = await User.objects.filter(status="active").first()
```

### IntegrityError

```python
class IntegrityError(ManagerError):
    """Raised when database integrity constraints are violated."""
```

**When raised:**

- Primary key violation (duplicate PK)
- Unique constraint violation
- Foreign key constraint violation
- Check constraint violation

```python
try:
    await User.objects.create(email="duplicate@example.com")
except IntegrityError as e:
    print(f"Constraint violation: {e}")
    # May contain: "UNIQUE constraint failed: users.email"
```

**Common patterns:**

```python
# Handle duplicate key
try:
    user = await User.objects.create(email=email)
except IntegrityError:
    user = await User.objects.get(email=email)

# Or use get_or_create
user, created = await User.objects.get_or_create(
    email=email,
    defaults={"name": name}
)
```

## Error Handling Patterns

### API Endpoint Pattern

```python
from fastapi import HTTPException
from oxyde import NotFoundError, IntegrityError, OxydeError

@app.get("/users/{user_id}")
async def get_user(user_id: int):
    try:
        return await User.objects.get(id=user_id)
    except NotFoundError:
        raise HTTPException(404, "User not found")

@app.post("/users")
async def create_user(data: UserCreate):
    try:
        return await User.objects.create(**data.model_dump())
    except IntegrityError:
        raise HTTPException(409, "User already exists")
    except OxydeError as e:
        raise HTTPException(500, str(e))
```

### Transaction Rollback Pattern

```python
from oxyde import IntegrityError
from oxyde.db import transaction

async def transfer(from_id: int, to_id: int, amount: Decimal):
    try:
        async with transaction.atomic():
            from_user = await User.objects.filter(id=from_id).for_update().first()
            to_user = await User.objects.filter(id=to_id).for_update().first()

            if from_user is None or to_user is None:
                raise NotFoundError("User not found")

            if from_user.balance < amount:
                raise ValueError("Insufficient balance")

            await User.objects.filter(id=from_id).update(balance=F("balance") - amount)
            await User.objects.filter(id=to_id).update(balance=F("balance") + amount)

    except IntegrityError:
        # Transaction automatically rolled back
        raise ValueError("Transfer failed due to constraint violation")
```

### Graceful Degradation Pattern

```python
async def get_user_with_fallback(user_id: int) -> User | None:
    try:
        return await User.objects.get(id=user_id)
    except NotFoundError:
        return None
    except OxydeError as e:
        logger.warning(f"Database error: {e}")
        return await cache.get(f"user:{user_id}")
```

## Import Reference

```python
from oxyde import (
    OxydeError,
    FieldError,
    FieldLookupError,
    FieldLookupValueError,
    ManagerError,
    NotFoundError,
    MultipleObjectsReturned,
    IntegrityError,
)
```

Or import from exceptions module:

```python
from oxyde.exceptions import (
    OxydeError,
    FieldError,
    FieldLookupError,
    FieldLookupValueError,
    ManagerError,
    NotFoundError,
    MultipleObjectsReturned,
    IntegrityError,
)
```

## Next Steps

- [Models](models.md) — Model definition
- [Queries](queries.md) — Query API
- [Transactions](transactions.md) — Transaction handling

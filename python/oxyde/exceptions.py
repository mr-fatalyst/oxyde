"""Exception hierarchy for Oxyde ORM.

All Oxyde exceptions inherit from OxydeError, allowing catch-all handling:

    try:
        user = await User.objects.get(id=999)
    except OxydeError as e:
        print(f"ORM error: {e}")

Exception hierarchy:
    OxydeError (base)
    ├── FieldError                - Invalid field definition or access
    ├── FieldLookupError          - Unknown lookup operator (e.g., __xyz)
    │   └── FieldLookupValueError - Invalid value for lookup (e.g., __in=None)
    └── ManagerError              - Query execution errors
        ├── NotFoundError         - get() returned no rows
        ├── MultipleObjectsReturned - get() returned multiple rows
        └── IntegrityError        - Constraint violation (PK, FK, UNIQUE)
"""

from __future__ import annotations


class OxydeError(Exception):
    """Base exception for all Oxyde-related errors."""


class FieldError(OxydeError):
    """Raised when a model field is invalid or missing."""


class FieldLookupError(OxydeError):
    """Raised when an unsupported field lookup is requested."""


class FieldLookupValueError(FieldLookupError):
    """Raised when the lookup value is not compatible with the operator."""


class ManagerError(OxydeError):
    """Raised for issues inside the ORM manager layer."""


class NotFoundError(ManagerError):
    """Raised when a query expecting a single row finds none."""


class MultipleObjectsReturned(ManagerError):
    """Raised when a query expecting a single row finds more than one."""


class IntegrityError(ManagerError):
    """Raised when database integrity constraints are violated."""


__all__ = [
    "OxydeError",
    "FieldError",
    "FieldLookupError",
    "FieldLookupValueError",
    "ManagerError",
    "NotFoundError",
    "MultipleObjectsReturned",
    "IntegrityError",
]

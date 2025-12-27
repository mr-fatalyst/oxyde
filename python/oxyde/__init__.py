"""Oxyde ORM - High-performance async Python ORM with Rust core.

This is the main entry point for the Oxyde ORM library. It re-exports
all public APIs from submodules for convenient access.

Public API:
    Models:
        OxydeModel: Base class for all ORM models (Pydantic v2 based).
        Field: Field configuration with db_pk, db_index, db_default, etc.
        Index: Composite index decorator for models.
        Check: CHECK constraint decorator for models.

    Queries:
        Query: SELECT query builder with Django-like filter syntax.
        QueryManager: Model.objects manager for CRUD operations.
        Q: Boolean expressions for complex filters (AND/OR/NOT).
        F: Database-side field references for expressions.

    Aggregates:
        Count, Sum, Avg, Max, Min: SQL aggregate functions.
        Concat, Coalesce: SQL scalar functions.
        RawSQL: Raw SQL expression wrapper.

    Database:
        db: Database module with init(), close(), connect(), lifespan().
        AsyncDatabase: Connection pool wrapper.
        PoolSettings: Pool configuration (max_connections, timeouts).
        atomic: Transaction context manager (decorator or async with).

    Exceptions:
        OxydeError: Base exception for all Oxyde errors.
        NotFoundError: Raised when get() finds no results.
        MultipleObjectsReturned: Raised when get() finds multiple results.
        IntegrityError: Raised on constraint violations.
        FieldError, FieldLookupError, ManagerError: Validation errors.

Example:
    from oxyde import OxydeModel, Field, db

    class User(OxydeModel):
        id: int | None = Field(default=None, db_pk=True)
        name: str

        class Meta:
            is_table = True

    async with db.connect("sqlite://app.db"):
        user = await User.objects.create(name="Alice")
        users = await User.objects.filter(name__startswith="A").all()
"""

from oxyde import db
from oxyde.db import (
    AsyncDatabase,
    PoolSettings,
    TransactionTimeoutError,
    atomic,
    disconnect_all,
    get_connection,
    register_connection,
)
from oxyde.exceptions import (
    FieldError,
    FieldLookupError,
    FieldLookupValueError,
    IntegrityError,
    ManagerError,
    MultipleObjectsReturned,
    NotFoundError,
    OxydeError,
)
from oxyde.models import Check, Field, Index, OxydeModel
from oxyde.queries import (
    Avg,
    Coalesce,
    Concat,
    Count,
    F,
    Max,
    Min,
    Q,
    Query,
    QueryManager,
    RawSQL,
    Sum,
    execute_raw,
)

__version__ = "0.3.0"

__all__ = [
    "OxydeModel",
    "db",
    "AsyncDatabase",
    "PoolSettings",
    "TransactionTimeoutError",
    "register_connection",
    "get_connection",
    "disconnect_all",
    "Field",
    "Index",
    "Check",
    "Query",
    "QueryManager",
    "OxydeError",
    "FieldError",
    "FieldLookupError",
    "FieldLookupValueError",
    "ManagerError",
    "NotFoundError",
    "MultipleObjectsReturned",
    "IntegrityError",
    "atomic",
    "F",
    "Q",
    "Count",
    "Sum",
    "Avg",
    "Max",
    "Min",
    "Concat",
    "Coalesce",
    "RawSQL",
    "execute_raw",
]

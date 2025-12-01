"""Database type mappings for different SQL dialects."""

from __future__ import annotations

from datetime import date, datetime, time, timedelta
from decimal import Decimal
from uuid import UUID

# SQLite type mappings
# https://www.sqlite.org/datatype3.html
SQLITE_TYPES = {
    "primary": {
        int: "INTEGER",
        str: "TEXT",
        float: "REAL",
        bool: "INTEGER",  # SQLite stores booleans as 0/1
        bytes: "BLOB",
        # Date/time types - SQLite stores as TEXT in ISO format
        datetime: "TEXT",
        date: "TEXT",
        time: "TEXT",
        timedelta: "TEXT",
        # Other types
        UUID: "TEXT",
        Decimal: "NUMERIC",
    },
    "valid": {
        # Integer types
        "INTEGER",
        "TINYINT",
        "SMALLINT",
        "MEDIUMINT",
        "BIGINT",
        "UNSIGNED BIG INT",
        "INT2",
        "INT8",
        # Text types
        "TEXT",
        "CHARACTER",
        "VARCHAR",
        "VARYING CHARACTER",
        "NCHAR",
        "NATIVE CHARACTER",
        "NVARCHAR",
        "CLOB",
        # Real types
        "REAL",
        "DOUBLE",
        "DOUBLE PRECISION",
        "FLOAT",
        # Numeric types
        "NUMERIC",
        "DECIMAL",
        "BOOLEAN",
        "DATE",
        "DATETIME",
        # Blob types
        "BLOB",
    },
}

# PostgreSQL type mappings
# https://www.postgresql.org/docs/current/datatype.html
POSTGRES_TYPES = {
    "primary": {
        int: "BIGINT",
        str: "TEXT",
        float: "DOUBLE PRECISION",
        bool: "BOOLEAN",
        bytes: "BYTEA",
        # Date/time types
        datetime: "TIMESTAMP",
        date: "DATE",
        time: "TIME",
        timedelta: "INTERVAL",
        # Other types
        UUID: "UUID",
        Decimal: "NUMERIC",
    },
    "valid": {
        # Integer types
        "SMALLINT",
        "INTEGER",
        "INT",
        "BIGINT",
        "SMALLSERIAL",
        "SERIAL",
        "BIGSERIAL",
        # Text types
        "TEXT",
        "VARCHAR",
        "CHAR",
        "CHARACTER",
        "CHARACTER VARYING",
        "BPCHAR",
        # Numeric types
        "NUMERIC",
        "DECIMAL",
        "REAL",
        "DOUBLE PRECISION",
        "FLOAT",
        "FLOAT4",
        "FLOAT8",
        "MONEY",
        # Boolean
        "BOOLEAN",
        "BOOL",
        # Binary
        "BYTEA",
        # Date/Time types
        "DATE",
        "TIME",
        "TIMESTAMP",
        "TIMESTAMPTZ",
        "TIMESTAMP WITH TIME ZONE",
        "TIMESTAMP WITHOUT TIME ZONE",
        "TIME WITH TIME ZONE",
        "TIME WITHOUT TIME ZONE",
        "INTERVAL",
        # JSON types
        "JSON",
        "JSONB",
        # UUID
        "UUID",
        # Network types
        "INET",
        "CIDR",
        "MACADDR",
        "MACADDR8",
        # Geometric types
        "POINT",
        "LINE",
        "LSEG",
        "BOX",
        "PATH",
        "POLYGON",
        "CIRCLE",
        # Arrays (generic)
        "ARRAY",
        # Range types
        "INT4RANGE",
        "INT8RANGE",
        "NUMRANGE",
        "TSRANGE",
        "TSTZRANGE",
        "DATERANGE",
        # Other
        "BIT",
        "BIT VARYING",
        "VARBIT",
        "XML",
        "TSVECTOR",
        "TSQUERY",
    },
}

# MySQL type mappings
# https://dev.mysql.com/doc/refman/8.0/en/data-types.html
MYSQL_TYPES = {
    "primary": {
        int: "BIGINT",
        str: "TEXT",
        float: "DOUBLE",
        bool: "TINYINT",  # MySQL stores booleans as TINYINT(1)
        bytes: "BLOB",
        # Date/time types
        datetime: "DATETIME",
        date: "DATE",
        time: "TIME",
        timedelta: "TIME",  # MySQL doesn't have INTERVAL
        # Other types
        UUID: "CHAR(36)",  # MySQL doesn't have native UUID
        Decimal: "DECIMAL",
    },
    "valid": {
        # Integer types
        "TINYINT",
        "SMALLINT",
        "MEDIUMINT",
        "INT",
        "INTEGER",
        "BIGINT",
        # Decimal types
        "DECIMAL",
        "NUMERIC",
        "FLOAT",
        "DOUBLE",
        "DOUBLE PRECISION",
        "REAL",
        # Date/Time types
        "DATE",
        "TIME",
        "DATETIME",
        "TIMESTAMP",
        "YEAR",
        # String types
        "CHAR",
        "VARCHAR",
        "BINARY",
        "VARBINARY",
        "TINYBLOB",
        "BLOB",
        "MEDIUMBLOB",
        "LONGBLOB",
        "TINYTEXT",
        "TEXT",
        "MEDIUMTEXT",
        "LONGTEXT",
        # JSON
        "JSON",
        # Spatial types
        "GEOMETRY",
        "POINT",
        "LINESTRING",
        "POLYGON",
        "MULTIPOINT",
        "MULTILINESTRING",
        "MULTIPOLYGON",
        "GEOMETRYCOLLECTION",
        # Enum/Set
        "ENUM",
        "SET",
        # Boolean (alias for TINYINT(1))
        "BOOLEAN",
        "BOOL",
        # Bit
        "BIT",
    },
}

DIALECT_MAPPINGS = {
    "sqlite": SQLITE_TYPES,
    "postgres": POSTGRES_TYPES,
    "mysql": MYSQL_TYPES,
}


def get_sql_type(
    python_type: type,
    dialect: str = "sqlite",
    is_pk: bool = False,
    db_type: str | None = None,
) -> str:
    """Get SQL type for Python type and dialect.

    Implements the type mapping logic from PK_TYPE_MAPPING_IMPLEMENTATION.md:
    1. If db_type is specified → use it (with translation for cross-platform)
    2. If int + is_pk → use native auto-increment type (SERIAL, INT, INTEGER)
    3. Otherwise → standard type mapping (is_pk doesn't affect type)

    Args:
        python_type: Python type (int, str, float, bool, bytes)
        dialect: Database dialect (sqlite, postgres, mysql)
        is_pk: Is this a primary key field?
        db_type: Explicit SQL type override from user

    Returns:
        SQL type string

    Raises:
        ValueError: If dialect is unknown or type not mapped
    """
    if dialect not in DIALECT_MAPPINGS:
        raise ValueError(
            f"Unknown dialect: {dialect}. Valid: {list(DIALECT_MAPPINGS.keys())}"
        )

    # 1. Explicit db_type - translate if needed
    if db_type:
        return translate_db_specific_type(db_type, dialect)

    # 2. Special case: int PK (auto-increment types)
    # ONLY int depends on is_pk, all other types don't
    if python_type is int and is_pk:
        if dialect == "postgres":
            return "SERIAL"
        elif dialect == "mysql":
            return "INT"  # AUTO_INCREMENT added separately
        elif dialect == "sqlite":
            return "INTEGER"

    # 3. Standard mapping (is_pk doesn't affect type)
    mapping = DIALECT_MAPPINGS[dialect]["primary"]

    if python_type not in mapping:
        raise ValueError(
            f"No SQL type mapping for Python type {python_type.__name__} in {dialect}"
        )

    return mapping[python_type]


def translate_db_specific_type(db_type: str, dialect: str) -> str:
    """Translate database-specific types for cross-platform compatibility.

    E.g., SERIAL/BIGSERIAL (PostgreSQL) → INT/BIGINT (MySQL) → INTEGER (SQLite)

    Args:
        db_type: User-specified SQL type
        dialect: Target dialect

    Returns:
        Translated SQL type
    """
    db_type_upper = db_type.upper()

    # PostgreSQL-specific types for other databases
    translations = {
        "postgres": {
            # No translation needed for native dialect
        },
        "mysql": {
            "SERIAL": "INT",
            "BIGSERIAL": "BIGINT",
        },
        "sqlite": {
            "SERIAL": "INTEGER",
            "BIGSERIAL": "INTEGER",
        },
    }

    if dialect not in translations:
        # Unknown dialect - return as-is
        return db_type

    dialect_translations = translations[dialect]
    return dialect_translations.get(db_type_upper, db_type)


def validate_sql_type(sql_type: str, dialect: str = "sqlite") -> bool:
    """Check if SQL type is valid for dialect.

    Args:
        sql_type: SQL type string (case-insensitive)
        dialect: Database dialect (sqlite, postgres, mysql)

    Returns:
        True if valid, False otherwise
    """
    if dialect not in DIALECT_MAPPINGS:
        return False

    valid_types = DIALECT_MAPPINGS[dialect]["valid"]
    return sql_type.upper() in valid_types


def normalize_sql_type(sql_type: str, dialect: str = "sqlite") -> str:
    """Normalize SQL type to canonical form for dialect.

    Args:
        sql_type: SQL type string
        dialect: Database dialect

    Returns:
        Normalized SQL type

    Raises:
        ValueError: If type is not valid for dialect
    """
    if not validate_sql_type(sql_type, dialect):
        raise ValueError(f"Invalid SQL type '{sql_type}' for dialect '{dialect}'")

    # Return uppercase version (canonical form)
    return sql_type.upper()


__all__ = [
    "DIALECT_MAPPINGS",
    "SQLITE_TYPES",
    "POSTGRES_TYPES",
    "MYSQL_TYPES",
    "get_sql_type",
    "validate_sql_type",
    "normalize_sql_type",
    "translate_db_specific_type",
]

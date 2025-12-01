# Oxyde ORM Examples

This directory contains example scripts demonstrating Oxyde ORM features.

## Running Examples

```bash
# Activate virtual environment
source .venv/bin/activate

# Set database URL (optional, defaults to SQLite)
export DATABASE_URL=sqlite://demo.db

# Run an example
python examples/quickstart.py
```

## Examples

### 1. quickstart.py - Getting Started

Basic ORM usage covering:
- Database connection setup
- Model definition with relations (FK)
- CRUD operations via Manager API
- Joins and prefetch for related data
- Projections with values/values_list

**Best for:** First-time users learning the basics.

### 2. advanced_queries.py - Advanced Queries

Complex query features:
- Q expressions (AND, OR, NOT)
- exclude() for negation
- Lookups (gte, lte, icontains, in)
- Aggregates (Count, Sum, Avg, Max, Min)
- group_by() with aggregates
- exists() for existence checks
- F expressions for field references
- order_by(), distinct(), limit(), offset()

**Best for:** Users needing complex filtering and aggregation.

### 3. transactions.py - Transaction Handling

Transaction management:
- atomic() context manager
- Automatic commit on success
- Automatic rollback on exception
- Nested transactions via savepoints

**Best for:** Understanding ACID guarantees and transaction patterns.

## Database Support

Examples work with SQLite by default. For PostgreSQL:

```bash
export DATABASE_URL=postgresql://user:pass@localhost:5432/mydb
```

See `postgres_schema.sql` for PostgreSQL table definitions.

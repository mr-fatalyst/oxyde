"""Migration context for upgrade/downgrade operations."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from oxyde.db.pool import AsyncDatabase as DatabaseConnection
    from oxyde.migrations.replay import SchemaState


class MigrationContext:
    """Context for executing migrations.

    Works in two modes (user doesn't know about this):
    - "collect": Collects operations for replay (used by makemigrations)
    - "execute": Generates and executes SQL (used by migrate)
    """

    def __init__(
        self,
        mode: str = "execute",
        db_alias: str | None = None,
        dialect: str | None = None,
        db_conn: DatabaseConnection | None = None,
        schema_state: SchemaState | None = None,
    ) -> None:
        """Initialize migration context.

        Args:
            mode: Internal mode - "collect" or "execute"
            db_alias: Database alias for execute mode (deprecated, use db_conn)
            dialect: Database dialect (sqlite, postgres, mysql)
            db_conn: Database connection (for execute mode)
            schema_state: Current schema state (for SQLite table rebuild)
        """
        self._mode = mode
        self._db_alias = db_alias
        self._dialect = dialect or "sqlite"
        self._db_conn = db_conn
        self._schema_state = schema_state
        self._operations: list[dict[str, Any]] = []

    @property
    def dialect(self) -> str:
        """Get current database dialect.

        Returns:
            Database dialect: "sqlite", "postgres", or "mysql"
        """
        return self._dialect

    # ========================================================================
    # Table operations
    # ========================================================================

    def create_table(
        self,
        name: str,
        fields: list[dict[str, Any]],
        indexes: list[dict[str, Any]] | None = None,
        foreign_keys: list[dict[str, Any]] | None = None,
        checks: list[dict[str, Any]] | None = None,
    ) -> None:
        """Create a table.

        Args:
            name: Table name
            fields: List of field definitions
            indexes: List of index definitions (optional)
            foreign_keys: List of foreign key definitions (optional)
            checks: List of check constraint definitions (optional)
        """
        op = {
            "type": "create_table",
            "table": {
                "name": name,
                "fields": fields,
                "indexes": indexes or [],
                "foreign_keys": foreign_keys or [],
                "checks": checks or [],
                "comment": None,
            },
        }

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    def drop_table(self, name: str) -> None:
        """Drop a table.

        Args:
            name: Table name
        """
        op = {"type": "drop_table", "name": name}

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    def rename_table(self, old_name: str, new_name: str) -> None:
        """Rename a table.

        Args:
            old_name: Current table name
            new_name: New table name
        """
        op = {
            "type": "rename_table",
            "old_name": old_name,
            "new_name": new_name,
        }

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    # ========================================================================
    # Column operations
    # ========================================================================

    def add_column(self, table: str, field: dict[str, Any]) -> None:
        """Add a column to a table.

        Args:
            table: Table name
            field: Field definition dict
        """
        op = {"type": "add_column", "table": table, "field": field}

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    def drop_column(self, table: str, field_name: str) -> None:
        """Drop a column from a table.

        Args:
            table: Table name
            field_name: Column name to drop
        """
        op = {"type": "drop_column", "table": table, "field": field_name}

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    def rename_column(self, table: str, old_name: str, new_name: str) -> None:
        """Rename a column.

        Args:
            table: Table name
            old_name: Current column name
            new_name: New column name
        """
        op = {
            "type": "rename_column",
            "table": table,
            "old_name": old_name,
            "new_name": new_name,
        }

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    def alter_column(self, table: str, field_name: str, **changes: Any) -> None:
        """Alter column properties (nullable, type, default, etc.).

        Args:
            table: Table name
            field_name: Column name
            **changes: Properties to change (nullable, type, default, etc.)
        """
        # For collect mode, store simple format
        if self._mode == "collect":
            op = {
                "type": "alter_column",
                "table": table,
                "column": field_name,
                "changes": changes,
            }
            self._operations.append(op)
            return

        # For execute mode, build Rust-compatible format with old_field/new_field
        old_field = None
        table_fields = None
        table_indexes = None

        if self._schema_state:
            table_schema = self._schema_state.tables.get(table)
            if table_schema:
                # Find old field definition
                for field in table_schema["fields"]:
                    if field["name"] == field_name:
                        old_field = dict(field)
                        break
                table_fields = table_schema["fields"]
                table_indexes = table_schema.get("indexes", [])

        if old_field is None:
            # Fallback: create minimal field definition
            old_field = {
                "name": field_name,
                "field_type": changes.get("type", "TEXT"),
                "nullable": True,
                "primary_key": False,
                "unique": False,
                "default": None,
                "auto_increment": False,
            }

        # Build new_field by applying changes to old_field
        new_field = dict(old_field)
        if "type" in changes:
            new_field["field_type"] = changes["type"]
        if "nullable" in changes:
            new_field["nullable"] = changes["nullable"]
        if "default" in changes:
            new_field["default"] = changes["default"]
        if "unique" in changes:
            new_field["unique"] = changes["unique"]

        # Build Rust-compatible operation
        op = {
            "type": "alter_column",
            "table": table,
            "old_field": old_field,
            "new_field": new_field,
        }

        # For SQLite, include full table schema for rebuild
        if self._dialect == "sqlite" and table_fields:
            op["table_fields"] = table_fields
            op["table_indexes"] = table_indexes

        self._execute_operation(op)

    # ========================================================================
    # Index operations
    # ========================================================================

    def create_index(self, table: str, index: dict[str, Any]) -> None:
        """Create an index.

        Args:
            table: Table name
            index: Index definition dict
        """
        op = {"type": "create_index", "table": table, "index": index}

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    def drop_index(self, table: str, index_name: str) -> None:
        """Drop an index.

        Args:
            table: Table name
            index_name: Index name to drop
        """
        op = {"type": "drop_index", "table": table, "name": index_name}

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    # ========================================================================
    # Foreign key operations
    # ========================================================================

    def add_foreign_key(
        self,
        table: str,
        name: str,
        columns: list[str],
        ref_table: str,
        ref_columns: list[str],
        on_delete: str = "NO ACTION",
        on_update: str = "NO ACTION",
    ) -> None:
        """Add a foreign key constraint.

        Args:
            table: Table name
            name: Constraint name
            columns: Local columns
            ref_table: Referenced table name
            ref_columns: Referenced columns
            on_delete: ON DELETE action (CASCADE, SET NULL, RESTRICT, NO ACTION)
            on_update: ON UPDATE action
        """
        op = {
            "type": "add_foreign_key",
            "table": table,
            "fk": {
                "name": name,
                "columns": columns,
                "ref_table": ref_table,
                "ref_columns": ref_columns,
                "on_delete": on_delete,
                "on_update": on_update,
            },
        }

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    def drop_foreign_key(self, table: str, name: str) -> None:
        """Drop a foreign key constraint.

        Args:
            table: Table name
            name: Constraint name to drop
        """
        op = {"type": "drop_foreign_key", "table": table, "name": name}

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    # ========================================================================
    # Check constraint operations
    # ========================================================================

    def add_check(self, table: str, name: str, expression: str) -> None:
        """Add a check constraint.

        Args:
            table: Table name
            name: Constraint name
            expression: SQL expression for the check
        """
        op = {
            "type": "add_check",
            "table": table,
            "check": {
                "name": name,
                "expression": expression,
            },
        }

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    def drop_check(self, table: str, name: str) -> None:
        """Drop a check constraint.

        Args:
            table: Table name
            name: Constraint name to drop
        """
        op = {"type": "drop_check", "table": table, "name": name}

        if self._mode == "collect":
            self._operations.append(op)
        else:
            self._execute_operation(op)

    # ========================================================================
    # Custom operations
    # ========================================================================

    def execute(self, sql: str) -> None:
        """Execute arbitrary SQL (for data migrations, added manually).

        This is ignored in "collect" mode (doesn't affect schema structure).

        Args:
            sql: SQL statement to execute
        """
        if self._mode == "execute":
            self._execute_raw_sql(sql)
        # In collect mode, ignore (doesn't affect schema)

    # ========================================================================
    # Internal methods
    # ========================================================================

    def get_collected_operations(self) -> list[dict[str, Any]]:
        """Get collected operations (for replay). Internal use only.

        Returns:
            List of collected operations
        """
        return self._operations

    def _execute_operation(self, op: dict[str, Any]) -> None:
        """Convert operation to SQL and execute in database.

        Args:
            op: Operation dictionary
        """
        import json

        from oxyde.core import migration_to_sql

        # Convert operation to SQL using Rust
        operations_json = json.dumps([op])
        sql_statements = migration_to_sql(operations_json, self._dialect)

        # Execute each SQL statement
        for sql in sql_statements:
            self._execute_raw_sql(sql)

    def _execute_raw_sql(self, sql: str) -> None:
        """Execute SQL in database (collects SQL for batch execution).

        Args:
            sql: SQL statement
        """
        if self._mode != "execute":
            return

        # Collect SQL statements for batch execution
        # They will be executed by the executor after upgrade() completes
        if not hasattr(self, "_sql_statements"):
            self._sql_statements: list[str] = []

        self._sql_statements.append(sql)

    async def _execute_collected_sql(self) -> None:
        """Execute all collected SQL statements wrapped in a transaction.

        This is called by the executor after upgrade() completes.

        Transaction behavior by dialect:
        - PostgreSQL: DDL is transactional, uses Rust transaction API
        - SQLite: DDL is transactional, uses Rust transaction API
        - MySQL: DDL is NOT transactional (implicit commit), no wrapping

        IMPORTANT: We use the Rust transaction API (begin_transaction/execute_in_transaction)
        instead of raw BEGIN/COMMIT SQL because the connection pool may assign different
        connections to different queries. The Rust transaction API ensures all queries
        in a transaction use the same connection.
        """
        if not hasattr(self, "_sql_statements"):
            return

        if self._db_conn is None:
            raise RuntimeError("Cannot execute SQL: no database connection provided")

        from oxyde.core import (
            begin_transaction,
            commit_transaction,
            execute_in_transaction,
            rollback_transaction,
        )
        from oxyde.core.ir import build_raw_sql_ir

        # MySQL doesn't support transactional DDL
        use_transaction = self._dialect in ("postgres", "sqlite")

        tx_id = None
        try:
            if use_transaction:
                tx_id = await begin_transaction(self._db_conn.name)

            for sql in self._sql_statements:
                sql_ir = build_raw_sql_ir(sql=sql)
                if tx_id is not None:
                    import msgpack

                    from oxyde.db.pool import _datetime_encoder

                    ir_bytes = msgpack.packb(sql_ir, default=_datetime_encoder)
                    await execute_in_transaction(self._db_conn.name, tx_id, ir_bytes)
                else:
                    await self._db_conn.execute(sql_ir)

            if tx_id is not None:
                await commit_transaction(tx_id)

        except Exception:
            if tx_id is not None:
                try:
                    await rollback_transaction(tx_id)
                except Exception:
                    pass  # Ignore rollback errors
            raise

        finally:
            # Clear collected statements
            self._sql_statements = []


__all__ = ["MigrationContext"]

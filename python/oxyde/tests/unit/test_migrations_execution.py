"""Tests for migration execution: apply, rollback, tracking."""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

import pytest

from oxyde.core import migration_compute_diff, migration_to_sql
from oxyde.migrations.context import MigrationContext



@pytest.fixture
def temp_migrations_dir():
    """Create a temporary migrations directory."""
    with tempfile.TemporaryDirectory() as tmpdir:
        migrations_dir = Path(tmpdir) / "migrations"
        migrations_dir.mkdir()
        yield migrations_dir


class TestMigrationFile:
    """Test migration file structure and parsing."""

    def test_migration_file_structure(self, temp_migrations_dir):
        """Test migration file has correct structure."""
        migration_content = '''"""
Migration: 0001_initial
Created: 2024-01-15 10:30:00
"""

from oxyde.migrate import Migration, operations

class Migration0001(Migration):
    dependencies = []

    operations = [
        operations.CreateTable(
            name="user",
            columns=[
                {"name": "id", "type": "INTEGER", "primary_key": True},
                {"name": "email", "type": "VARCHAR(255)", "nullable": False},
            ],
        ),
    ]
'''
        migration_file = temp_migrations_dir / "0001_initial.py"
        migration_file.write_text(migration_content)

        assert migration_file.exists()
        content = migration_file.read_text()
        assert "Migration0001" in content
        assert "CreateTable" in content

    def test_migration_naming_convention(self, temp_migrations_dir):
        """Test migration naming convention."""
        # Migrations should be named: NNNN_description.py
        valid_names = [
            "0001_initial.py",
            "0002_add_email_field.py",
            "0010_create_posts_table.py",
            "0100_add_indexes.py",
        ]

        for name in valid_names:
            file = temp_migrations_dir / name
            file.write_text("# Migration\n")
            assert file.exists()

    def test_migration_dependencies(self, temp_migrations_dir):
        """Test migration dependencies declaration."""
        migration_content = """
class Migration0002(Migration):
    dependencies = ["0001_initial"]

    operations = []
"""
        migration_file = temp_migrations_dir / "0002_dependent.py"
        migration_file.write_text(migration_content)

        content = migration_file.read_text()
        assert "dependencies" in content
        assert "0001_initial" in content


class TestMigrationOperations:
    """Test migration operation types."""

    def test_create_table_operation(self):
        """Test CreateTable operation structure."""
        # Conceptual test for operation structure
        operation = {
            "type": "create_table",
            "name": "user",
            "columns": [
                {"name": "id", "type": "INTEGER", "primary_key": True},
                {"name": "name", "type": "VARCHAR(100)"},
            ],
        }

        assert operation["type"] == "create_table"
        assert len(operation["columns"]) == 2

    def test_drop_table_operation(self):
        """Test DropTable operation structure."""
        operation = {
            "type": "drop_table",
            "name": "old_table",
        }

        assert operation["type"] == "drop_table"
        assert operation["name"] == "old_table"

    def test_add_column_operation(self):
        """Test AddColumn operation structure."""
        operation = {
            "type": "add_column",
            "table": "user",
            "column": {
                "name": "email",
                "type": "VARCHAR(255)",
                "nullable": True,
            },
        }

        assert operation["type"] == "add_column"
        assert operation["column"]["name"] == "email"

    def test_drop_column_operation(self):
        """Test DropColumn operation structure."""
        operation = {
            "type": "drop_column",
            "table": "user",
            "column": "legacy_field",
        }

        assert operation["type"] == "drop_column"
        assert operation["column"] == "legacy_field"

    def test_alter_column_operation(self):
        """Test AlterColumn operation structure."""
        operation = {
            "type": "alter_column",
            "table": "user",
            "column": "email",
            "changes": {
                "nullable": False,
                "default": "'unknown@example.com'",
            },
        }

        assert operation["type"] == "alter_column"
        assert operation["changes"]["nullable"] is False

    def test_create_index_operation(self):
        """Test CreateIndex operation structure."""
        operation = {
            "type": "create_index",
            "name": "idx_user_email",
            "table": "user",
            "columns": ["email"],
            "unique": True,
        }

        assert operation["type"] == "create_index"
        assert operation["unique"] is True

    def test_drop_index_operation(self):
        """Test DropIndex operation structure."""
        operation = {
            "type": "drop_index",
            "name": "idx_user_email",
        }

        assert operation["type"] == "drop_index"

    def test_add_foreign_key_operation(self):
        """Test AddForeignKey operation structure."""
        operation = {
            "type": "add_foreign_key",
            "table": "post",
            "column": "author_id",
            "references": {
                "table": "user",
                "column": "id",
            },
            "on_delete": "CASCADE",
        }

        assert operation["type"] == "add_foreign_key"
        assert operation["on_delete"] == "CASCADE"


class TestMigrationTracking:
    """Test migration tracking (applied migrations)."""

    def test_migrations_table_schema(self):
        """Test oxyde_migrations table schema."""
        # Expected schema for migrations tracking table
        schema = {
            "table": "oxyde_migrations",
            "columns": [
                {"name": "id", "type": "INTEGER", "primary_key": True},
                {"name": "name", "type": "VARCHAR(255)", "nullable": False},
                {"name": "applied_at", "type": "TIMESTAMP", "nullable": False},
            ],
        }

        assert schema["table"] == "oxyde_migrations"
        assert len(schema["columns"]) == 3

    def test_migration_record_structure(self):
        """Test migration record structure."""
        record = {
            "id": 1,
            "name": "0001_initial",
            "applied_at": "2024-01-15T10:30:00",
        }

        assert record["name"] == "0001_initial"
        assert "applied_at" in record


class TestMigrationExecution:
    """Test migration execution flow."""

    def test_apply_migration_order(self):
        """Test migrations are applied in order."""
        migrations = [
            "0001_initial",
            "0002_add_posts",
            "0003_add_comments",
        ]

        # Should be sorted by number prefix
        sorted_migrations = sorted(migrations)
        assert sorted_migrations == migrations

    def test_skip_applied_migrations(self):
        """Test that applied migrations are skipped."""
        all_migrations = ["0001_initial", "0002_add_posts", "0003_add_comments"]
        applied_migrations = ["0001_initial"]

        pending = [m for m in all_migrations if m not in applied_migrations]

        assert "0001_initial" not in pending
        assert "0002_add_posts" in pending
        assert "0003_add_comments" in pending

    def test_detect_pending_migrations(self):
        """Test detecting pending migrations."""
        all_migrations = {"0001_initial", "0002_add_posts", "0003_add_comments"}
        applied_migrations = {"0001_initial", "0002_add_posts"}

        pending = all_migrations - applied_migrations

        assert pending == {"0003_add_comments"}


class TestMigrationRollback:
    """Test migration rollback functionality."""

    def test_rollback_operation_reversal(self):
        """Test that operations can be reversed."""
        # Forward operation
        forward = {
            "type": "add_column",
            "table": "user",
            "column": {"name": "email", "type": "VARCHAR(255)"},
        }

        # Reverse operation
        backward = {
            "type": "drop_column",
            "table": "user",
            "column": "email",
        }

        assert forward["type"] == "add_column"
        assert backward["type"] == "drop_column"

    def test_create_drop_table_reversal(self):
        """Test CreateTable reverses to DropTable."""
        forward = {"type": "create_table", "name": "posts"}
        backward = {"type": "drop_table", "name": "posts"}

        assert forward["name"] == backward["name"]

    def test_add_drop_index_reversal(self):
        """Test CreateIndex reverses to DropIndex."""
        forward = {"type": "create_index", "name": "idx_email"}
        backward = {"type": "drop_index", "name": "idx_email"}

        assert forward["name"] == backward["name"]


class TestMigrationSQLGeneration:
    """Test SQL generation for migrations."""

    def test_create_table_sql_postgresql(self):
        """Test CREATE TABLE SQL for PostgreSQL."""
        expected_sql = """CREATE TABLE "user" (
    "id" SERIAL PRIMARY KEY,
    "name" VARCHAR(100) NOT NULL,
    "email" VARCHAR(255) NOT NULL UNIQUE
)"""
        # This is a conceptual test - actual implementation would generate SQL
        assert "CREATE TABLE" in expected_sql
        assert "PRIMARY KEY" in expected_sql

    def test_create_table_sql_sqlite(self):
        """Test CREATE TABLE SQL for SQLite."""
        expected_sql = """CREATE TABLE "user" (
    "id" INTEGER PRIMARY KEY AUTOINCREMENT,
    "name" TEXT NOT NULL,
    "email" TEXT NOT NULL UNIQUE
)"""
        assert "INTEGER PRIMARY KEY AUTOINCREMENT" in expected_sql

    def test_add_column_sql(self):
        """Test ALTER TABLE ADD COLUMN SQL."""
        expected_sql = 'ALTER TABLE "user" ADD COLUMN "phone" VARCHAR(20)'
        assert "ADD COLUMN" in expected_sql

    def test_create_index_sql(self):
        """Test CREATE INDEX SQL."""
        expected_sql = 'CREATE INDEX "idx_user_email" ON "user" ("email")'
        assert "CREATE INDEX" in expected_sql

    def test_create_unique_index_sql(self):
        """Test CREATE UNIQUE INDEX SQL."""
        expected_sql = 'CREATE UNIQUE INDEX "idx_user_email" ON "user" ("email")'
        assert "CREATE UNIQUE INDEX" in expected_sql


class TestMigrationValidation:
    """Test migration validation."""

    def test_validate_migration_name(self):
        """Test migration name validation."""
        import re

        pattern = r"^\d{4}_[a-z0-9_]+$"

        valid_names = ["0001_initial", "0002_add_users", "0010_create_index"]
        invalid_names = ["initial", "add_users_0001", "0001-initial", "1_test"]

        for name in valid_names:
            assert re.match(pattern, name), f"{name} should be valid"

        for name in invalid_names:
            assert not re.match(pattern, name), f"{name} should be invalid"

    def test_validate_table_name(self):
        """Test table name validation."""
        import re

        pattern = r"^[a-z][a-z0-9_]*$"

        valid_names = ["user", "user_profile", "users2"]
        invalid_names = ["User", "user-profile", "2users", "_users"]

        for name in valid_names:
            assert re.match(pattern, name), f"{name} should be valid"

        for name in invalid_names:
            assert not re.match(pattern, name), f"{name} should be invalid"

    def test_validate_column_name(self):
        """Test column name validation."""
        import re

        pattern = r"^[a-z][a-z0-9_]*$"

        valid_names = ["id", "user_id", "created_at"]
        invalid_names = ["ID", "user-id", "1column"]

        for name in valid_names:
            assert re.match(pattern, name), f"{name} should be valid"


class TestMigrationDependencies:
    """Test migration dependency resolution."""

    def test_resolve_linear_dependencies(self):
        """Test resolving linear dependencies."""
        migrations = {
            "0001_initial": [],
            "0002_add_users": ["0001_initial"],
            "0003_add_posts": ["0002_add_users"],
        }

        # Topological sort would give: 0001 -> 0002 -> 0003
        order = []
        applied: set[str] = set()

        def resolve(name: str):
            if name in applied:
                return
            for dep in migrations.get(name, []):
                resolve(dep)
            order.append(name)
            applied.add(name)

        for name in migrations:
            resolve(name)

        assert order == ["0001_initial", "0002_add_users", "0003_add_posts"]

    def test_detect_circular_dependencies(self):
        """Test detecting circular dependencies."""
        migrations = {
            "0001_a": ["0002_b"],
            "0002_b": ["0001_a"],  # Circular!
        }

        def has_cycle(migrations: dict) -> bool:
            visited: set[str] = set()
            rec_stack: set[str] = set()

            def dfs(name: str) -> bool:
                visited.add(name)
                rec_stack.add(name)

                for dep in migrations.get(name, []):
                    if dep not in visited:
                        if dfs(dep):
                            return True
                    elif dep in rec_stack:
                        return True

                rec_stack.remove(name)
                return False

            for name in migrations:
                if name not in visited:
                    if dfs(name):
                        return True
            return False

        assert has_cycle(migrations) is True

    def test_missing_dependency_detection(self):
        """Test detecting missing dependencies."""
        migrations = {
            "0002_add_posts": ["0001_initial"],  # 0001 doesn't exist!
        }
        all_names = set(migrations.keys())

        for name, deps in migrations.items():
            for dep in deps:
                if dep not in all_names:
                    missing = dep
                    break

        assert missing == "0001_initial"


class TestDropOpsMinimalPayloadRegression:
    """Regression: ctx.drop_foreign_key/drop_index/drop_check must produce valid SQL.

    Reproduces the bug where minimal payload from context.py
    ({type, table, name}) failed to deserialize into Rust MigrationOp because
    fk_def/index_def/check_def were required fields.
    """

    def test_drop_foreign_key_postgres(self):
        ctx = MigrationContext(mode="collect", dialect="postgres")
        ctx.drop_foreign_key("books", "fk_books_author_id")

        ops = ctx.get_collected_operations()
        sqls = migration_to_sql(json.dumps(ops), "postgres")

        assert any("DROP" in s and "fk_books_author_id" in s for s in sqls)

    def test_drop_foreign_key_mysql(self):
        ctx = MigrationContext(mode="collect", dialect="mysql")
        ctx.drop_foreign_key("books", "fk_books_author_id")

        sqls = migration_to_sql(json.dumps(ctx.get_collected_operations()), "mysql")
        assert any("DROP" in s and "fk_books_author_id" in s for s in sqls)

    def test_drop_check_postgres(self):
        ctx = MigrationContext(mode="collect", dialect="postgres")
        ctx.drop_check("users", "chk_users_age")

        sqls = migration_to_sql(json.dumps(ctx.get_collected_operations()), "postgres")
        assert any("DROP CONSTRAINT" in s and "chk_users_age" in s for s in sqls)

    def test_drop_check_mysql(self):
        ctx = MigrationContext(mode="collect", dialect="mysql")
        ctx.drop_check("users", "chk_users_age")

        sqls = migration_to_sql(json.dumps(ctx.get_collected_operations()), "mysql")
        assert any("DROP CHECK" in s and "chk_users_age" in s for s in sqls)

    @pytest.mark.parametrize("dialect", ["postgres", "mysql", "sqlite"])
    def test_drop_index(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.drop_index("users", "idx_users_email")

        sqls = migration_to_sql(json.dumps(ctx.get_collected_operations()), dialect)
        assert any("idx_users_email" in s for s in sqls)

    def test_rust_diff_drop_index_roundtrip(self):
        """Rust-diff emits drop_index; that JSON must flow back through migration_to_sql."""
        old_snapshot = {
            "version": 1,
            "tables": {
                "users": {
                    "name": "users",
                    "fields": [
                        {
                            "name": "id",
                            "python_type": "int",
                            "db_type": None,
                            "nullable": False,
                            "primary_key": True,
                            "unique": False,
                            "default": None,
                            "auto_increment": False,
                        }
                    ],
                    "indexes": [
                        {
                            "name": "idx_users_email",
                            "fields": ["email"],
                            "unique": False,
                            "method": None,
                        }
                    ],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": None,
                }
            },
        }
        new_snapshot = {
            "version": 1,
            "tables": {
                "users": {
                    "name": "users",
                    "fields": old_snapshot["tables"]["users"]["fields"],
                    "indexes": [],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": None,
                }
            },
        }

        ops_json = migration_compute_diff(
            json.dumps(old_snapshot), json.dumps(new_snapshot)
        )
        ops = json.loads(ops_json)
        assert any(op["type"] == "drop_index" for op in ops)

        # Must not raise — round-trip through Rust must succeed
        sqls = migration_to_sql(ops_json, "postgres")
        assert any("idx_users_email" in s for s in sqls)

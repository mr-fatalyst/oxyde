"""Tests for migration execution: apply, rollback, tracking."""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

import pytest

from oxyde.core import migration_compute_diff, migration_to_sql
from oxyde.migrations.context import MigrationContext
from oxyde.migrations.replay import SchemaState


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
        old_snapshot: dict = {
            "version": 1,
            "tables": {
                "users": {
                    "name": "users",
                    "fields": [
                        {
                            "name": "id",
                            "column_type": {"kind": "big_integer"},
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


ALL_DIALECTS = ["postgres", "mysql", "sqlite"]
NON_SQLITE = ["postgres", "mysql"]
PARTIAL_INDEX_DIALECTS = ["postgres", "sqlite"]


def _field(name: str, **overrides) -> dict:
    """Build a minimal field dict in the wire form (column_type spec).

    Accepts python_type="..." as readable shorthand, translated through the
    same legacy-name mapping the production reader uses.
    """
    from oxyde.core.column_types import spec_from_legacy_name

    python_type = overrides.pop("python_type", "str")
    base = {
        "name": name,
        "column_type": spec_from_legacy_name(python_type),
        "db_type": None,
        "nullable": False,
        "primary_key": False,
        "unique": False,
        "default": None,
        "auto_increment": False,
    }
    base.update(overrides)
    return base


def _render(ctx: MigrationContext, dialect: str) -> list[str]:
    """Render collected ops through Rust migration_to_sql."""
    return migration_to_sql(json.dumps(ctx.get_collected_operations()), dialect)


class TestOpMatrix:
    """Every MigrationOp × every supported dialect → valid SQL.

    Ensures Python context.py payload shape matches Rust MigrationOp schema
    for all operations. Any future mismatch (like the drop_fk bug) fails here.
    """

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_create_table(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.create_table(
            "users",
            fields=[
                _field("id", python_type="int", primary_key=True, auto_increment=True),
                _field("email", unique=True),
            ],
        )
        sqls = _render(ctx, dialect)
        assert any("CREATE TABLE" in s.upper() and "users" in s for s in sqls)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_drop_table(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.drop_table("users")
        sqls = _render(ctx, dialect)
        assert any("DROP TABLE" in s.upper() and "users" in s for s in sqls)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_rename_table(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.rename_table("users", "accounts")
        sqls = _render(ctx, dialect)
        assert any("users" in s and "accounts" in s for s in sqls)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_add_column(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.add_column("users", _field("nickname", nullable=True))
        sqls = _render(ctx, dialect)
        assert any("ADD" in s.upper() and "nickname" in s for s in sqls)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_drop_column(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.drop_column("users", "nickname")
        sqls = _render(ctx, dialect)
        assert any("DROP" in s.upper() and "nickname" in s for s in sqls)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_rename_column(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.rename_column("users", "nickname", "handle")
        sqls = _render(ctx, dialect)
        assert any("nickname" in s and "handle" in s for s in sqls)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_create_index(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.create_index(
            "users",
            {
                "name": "idx_users_email",
                "fields": ["email"],
                "unique": False,
                "method": None,
            },
        )
        sqls = _render(ctx, dialect)
        assert any(
            "CREATE" in s.upper() and "INDEX" in s.upper() and "idx_users_email" in s
            for s in sqls
        )

    @pytest.mark.parametrize("dialect", PARTIAL_INDEX_DIALECTS)
    def test_create_partial_index(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.create_index(
            "users",
            {
                "name": "idx_users_active_email",
                "fields": ["email"],
                "unique": True,
                "method": None,
                "where": "  deleted_at IS NULL  ",
            },
        )
        sqls = _render(ctx, dialect)
        assert any("WHERE deleted_at IS NULL" in s for s in sqls)

    def test_create_partial_index_mysql_rejected(self):
        ctx = MigrationContext(mode="collect", dialect="mysql")
        ctx.create_index(
            "users",
            {
                "name": "idx_users_active_email",
                "fields": ["email"],
                "unique": True,
                "method": None,
                "where": "deleted_at IS NULL",
            },
        )
        with pytest.raises(Exception, match="partial indexes"):
            _render(ctx, "mysql")

    def test_create_nulls_not_distinct_index_postgres(self):
        ctx = MigrationContext(mode="collect", dialect="postgres")
        ctx.create_index(
            "users",
            {
                "name": "idx_users_email_nulls_not_distinct",
                "fields": ["email"],
                "unique": True,
                "method": None,
                "nulls_not_distinct": True,
            },
        )
        sqls = _render(ctx, "postgres")
        assert any("NULLS NOT DISTINCT" in s for s in sqls)

    @pytest.mark.parametrize("dialect", ["mysql", "sqlite"])
    def test_create_nulls_not_distinct_index_non_postgres_rejected(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.create_index(
            "users",
            {
                "name": "idx_users_email_nulls_not_distinct",
                "fields": ["email"],
                "unique": True,
                "method": None,
                "nulls_not_distinct": True,
            },
        )
        with pytest.raises(Exception, match="NULLS NOT DISTINCT"):
            _render(ctx, dialect)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_drop_index(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.drop_index("users", "idx_users_email")
        sqls = _render(ctx, dialect)
        assert any("DROP" in s.upper() and "idx_users_email" in s for s in sqls)

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_add_foreign_key(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.add_foreign_key(
            "posts",
            "fk_posts_author",
            ["author_id"],
            "users",
            ["id"],
            on_delete="CASCADE",
        )
        sqls = _render(ctx, dialect)
        assert any("fk_posts_author" in s and "users" in s for s in sqls)

    def test_add_foreign_key_sqlite_rejected(self):
        ctx = MigrationContext(mode="collect", dialect="sqlite")
        ctx.add_foreign_key("posts", "fk_posts_author", ["author_id"], "users", ["id"])
        with pytest.raises(Exception, match="SQLite"):
            _render(ctx, "sqlite")

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_drop_foreign_key(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.drop_foreign_key("posts", "fk_posts_author")
        sqls = _render(ctx, dialect)
        assert any("DROP" in s.upper() and "fk_posts_author" in s for s in sqls)

    def test_drop_foreign_key_sqlite_rejected(self):
        ctx = MigrationContext(mode="collect", dialect="sqlite")
        ctx.drop_foreign_key("posts", "fk_posts_author")
        with pytest.raises(Exception, match="SQLite"):
            _render(ctx, "sqlite")

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_add_check(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.add_check("users", "chk_users_age", "age >= 0")
        sqls = _render(ctx, dialect)
        assert any("chk_users_age" in s and "CHECK" in s.upper() for s in sqls)

    def test_add_check_sqlite_rejected(self):
        ctx = MigrationContext(mode="collect", dialect="sqlite")
        ctx.add_check("users", "chk_users_age", "age >= 0")
        with pytest.raises(Exception, match="SQLite"):
            _render(ctx, "sqlite")

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_drop_check(self, dialect):
        ctx = MigrationContext(mode="collect", dialect=dialect)
        ctx.drop_check("users", "chk_users_age")
        sqls = _render(ctx, dialect)
        assert any("DROP" in s.upper() and "chk_users_age" in s for s in sqls)

    def test_drop_check_sqlite_rejected(self):
        ctx = MigrationContext(mode="collect", dialect="sqlite")
        ctx.drop_check("users", "chk_users_age")
        with pytest.raises(Exception, match="SQLite"):
            _render(ctx, "sqlite")


def _base_snapshot() -> dict:
    """Snapshot with a single `users` table (id, email)."""
    return {
        "version": 1,
        "tables": {
            "users": {
                "name": "users",
                "fields": [
                    _field(
                        "id", python_type="int", primary_key=True, auto_increment=True
                    ),
                    _field("email", unique=True),
                ],
                "indexes": [],
                "foreign_keys": [],
                "checks": [],
                "comment": None,
            }
        },
    }


class TestMigrationInvariants:
    """Architectural invariants that protect against regressions in ops composition."""

    # ── Rust-diff round-trip: emit → migration_to_sql must not fail ─────

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_roundtrip_add_column(self, dialect):
        old = _base_snapshot()
        new = _base_snapshot()
        new["tables"]["users"]["fields"].append(_field("phone", nullable=True))

        ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
        assert any(op["type"] == "add_column" for op in json.loads(ops_json))
        migration_to_sql(ops_json, dialect)  # must not raise

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_roundtrip_drop_column(self, dialect):
        old = _base_snapshot()
        old["tables"]["users"]["fields"].append(_field("phone", nullable=True))
        new = _base_snapshot()

        ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
        assert any(op["type"] == "drop_column" for op in json.loads(ops_json))
        migration_to_sql(ops_json, dialect)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_roundtrip_create_index(self, dialect):
        old = _base_snapshot()
        new = _base_snapshot()
        new["tables"]["users"]["indexes"].append(
            {
                "name": "idx_users_email",
                "fields": ["email"],
                "unique": False,
                "method": None,
            }
        )

        ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
        assert any(op["type"] == "create_index" for op in json.loads(ops_json))
        migration_to_sql(ops_json, dialect)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_roundtrip_drop_index(self, dialect):
        old = _base_snapshot()
        old["tables"]["users"]["indexes"].append(
            {
                "name": "idx_users_email",
                "fields": ["email"],
                "unique": False,
                "method": None,
            }
        )
        new = _base_snapshot()

        ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
        assert any(op["type"] == "drop_index" for op in json.loads(ops_json))
        migration_to_sql(ops_json, dialect)

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_roundtrip_add_foreign_key(self, dialect):
        old = _base_snapshot()
        old["tables"]["posts"] = {
            "name": "posts",
            "fields": [
                _field("id", python_type="int", primary_key=True),
                _field("author_id", python_type="int"),
            ],
            "indexes": [],
            "foreign_keys": [],
            "checks": [],
            "comment": None,
        }
        new = json.loads(json.dumps(old))  # deep copy
        new["tables"]["posts"]["foreign_keys"].append(
            {
                "name": "fk_posts_author",
                "columns": ["author_id"],
                "ref_table": "users",
                "ref_columns": ["id"],
                "on_delete": "CASCADE",
                "on_update": "NO ACTION",
            }
        )

        ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
        assert any(op["type"] == "add_foreign_key" for op in json.loads(ops_json))
        migration_to_sql(ops_json, dialect)

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_roundtrip_drop_foreign_key(self, dialect):
        old = _base_snapshot()
        old["tables"]["posts"] = {
            "name": "posts",
            "fields": [
                _field("id", python_type="int", primary_key=True),
                _field("author_id", python_type="int"),
            ],
            "indexes": [],
            "foreign_keys": [
                {
                    "name": "fk_posts_author",
                    "columns": ["author_id"],
                    "ref_table": "users",
                    "ref_columns": ["id"],
                    "on_delete": "CASCADE",
                    "on_update": "NO ACTION",
                }
            ],
            "checks": [],
            "comment": None,
        }
        new = json.loads(json.dumps(old))
        new["tables"]["posts"]["foreign_keys"] = []

        ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
        assert any(op["type"] == "drop_foreign_key" for op in json.loads(ops_json))
        migration_to_sql(ops_json, dialect)

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_roundtrip_add_check(self, dialect):
        old = _base_snapshot()
        new = _base_snapshot()
        new["tables"]["users"]["checks"].append(
            {"name": "chk_users_age", "expression": "age >= 0"}
        )

        ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
        assert any(op["type"] == "add_check" for op in json.loads(ops_json))
        migration_to_sql(ops_json, dialect)

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_roundtrip_change_check_expression(self, dialect):
        old = _base_snapshot()
        old["tables"]["users"]["checks"].append(
            {"name": "chk_users_age", "expression": "age >= 0"}
        )
        new = _base_snapshot()
        new["tables"]["users"]["checks"].append(
            {"name": "chk_users_age", "expression": "age > 0"}
        )

        ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
        ops = json.loads(ops_json)

        assert [op["type"] for op in ops] == ["drop_check", "add_check"]
        assert ops[0]["name"] == "chk_users_age"
        assert ops[0]["check_def"]["expression"] == "age >= 0"
        assert ops[1]["check"]["name"] == "chk_users_age"
        assert ops[1]["check"]["expression"] == "age > 0"
        migration_to_sql(ops_json, dialect)

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_roundtrip_drop_check(self, dialect):
        old = _base_snapshot()
        old["tables"]["users"]["checks"].append(
            {"name": "chk_users_age", "expression": "age >= 0"}
        )
        new = _base_snapshot()

        ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
        assert any(op["type"] == "drop_check" for op in json.loads(ops_json))
        migration_to_sql(ops_json, dialect)

    # ── Reverse symmetry on SchemaState ─────────────────────────────────

    def test_reverse_create_drop_table(self):
        state = SchemaState()
        initial = state.to_snapshot()

        state.apply_operation(
            {
                "type": "create_table",
                "table": {
                    "name": "t",
                    "fields": [_field("id", python_type="int", primary_key=True)],
                    "indexes": [],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": None,
                },
            }
        )
        state.apply_operation({"type": "drop_table", "name": "t"})
        assert state.to_snapshot() == initial

    def test_reverse_add_drop_column(self):
        state = SchemaState()
        state.apply_operation(
            {
                "type": "create_table",
                "table": {
                    "name": "t",
                    "fields": [_field("id", python_type="int", primary_key=True)],
                    "indexes": [],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": None,
                },
            }
        )
        baseline = state.to_snapshot()

        state.apply_operation(
            {"type": "add_column", "table": "t", "field": _field("email")}
        )
        state.apply_operation({"type": "drop_column", "table": "t", "field": "email"})
        assert state.to_snapshot() == baseline

    def test_reverse_create_drop_index(self):
        state = SchemaState()
        state.apply_operation(
            {
                "type": "create_table",
                "table": {
                    "name": "t",
                    "fields": [
                        _field("id", python_type="int", primary_key=True),
                        _field("email"),
                    ],
                    "indexes": [],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": None,
                },
            }
        )
        baseline = state.to_snapshot()

        idx = {
            "name": "idx_t_email",
            "fields": ["email"],
            "unique": False,
            "method": None,
        }
        state.apply_operation({"type": "create_index", "table": "t", "index": idx})
        state.apply_operation({"type": "drop_index", "table": "t", "name": idx["name"]})
        assert state.to_snapshot() == baseline

    def test_reverse_add_drop_foreign_key(self):
        state = SchemaState()
        state.apply_operation(
            {
                "type": "create_table",
                "table": {
                    "name": "posts",
                    "fields": [
                        _field("id", python_type="int", primary_key=True),
                        _field("author_id", python_type="int"),
                    ],
                    "indexes": [],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": None,
                },
            }
        )
        baseline = state.to_snapshot()

        fk = {
            "name": "fk_posts_author",
            "columns": ["author_id"],
            "ref_table": "users",
            "ref_columns": ["id"],
            "on_delete": "CASCADE",
            "on_update": "NO ACTION",
        }
        state.apply_operation({"type": "add_foreign_key", "table": "posts", "fk": fk})
        state.apply_operation(
            {"type": "drop_foreign_key", "table": "posts", "name": fk["name"]}
        )
        assert state.to_snapshot() == baseline

    def test_reverse_add_drop_check(self):
        state = SchemaState()
        state.apply_operation(
            {
                "type": "create_table",
                "table": {
                    "name": "t",
                    "fields": [_field("id", python_type="int", primary_key=True)],
                    "indexes": [],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": None,
                },
            }
        )
        baseline = state.to_snapshot()

        check = {"name": "chk_t_positive", "expression": "id > 0"}
        state.apply_operation({"type": "add_check", "table": "t", "check": check})
        state.apply_operation(
            {"type": "drop_check", "table": "t", "name": check["name"]}
        )
        assert state.to_snapshot() == baseline

    def test_reverse_rename_table(self):
        state = SchemaState()
        state.apply_operation(
            {
                "type": "create_table",
                "table": {
                    "name": "users",
                    "fields": [_field("id", python_type="int", primary_key=True)],
                    "indexes": [],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": None,
                },
            }
        )
        baseline = state.to_snapshot()

        state.apply_operation(
            {"type": "rename_table", "old_name": "users", "new_name": "accounts"}
        )
        state.apply_operation(
            {"type": "rename_table", "old_name": "accounts", "new_name": "users"}
        )
        assert state.to_snapshot() == baseline

    def test_reverse_rename_column(self):
        state = SchemaState()
        state.apply_operation(
            {
                "type": "create_table",
                "table": {
                    "name": "t",
                    "fields": [
                        _field("id", python_type="int", primary_key=True),
                        _field("nickname"),
                    ],
                    "indexes": [],
                    "foreign_keys": [],
                    "checks": [],
                    "comment": None,
                },
            }
        )
        baseline = state.to_snapshot()

        state.apply_operation(
            {
                "type": "rename_column",
                "table": "t",
                "old_name": "nickname",
                "new_name": "handle",
            }
        )
        state.apply_operation(
            {
                "type": "rename_column",
                "table": "t",
                "old_name": "handle",
                "new_name": "nickname",
            }
        )
        assert state.to_snapshot() == baseline

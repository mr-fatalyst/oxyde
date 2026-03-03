"""Integration tests for migration system.

Tests cover:
- Migration file generation
- Migration replay (schema state building)
- Dependency resolution and topological sorting
- Forward/backward migration execution
"""

from __future__ import annotations

from pathlib import Path

import pytest

# =============================================================================
# Generator Tests
# =============================================================================


class TestMigrationGenerator:
    """Test migration file generation."""

    def test_generate_create_table_migration(self, tmp_path: Path):
        """Test generating migration for create_table operation."""
        from oxyde.migrations.generator import generate_migration_file

        operations = [
            {
                "type": "create_table",
                "table": {
                    "name": "users",
                    "fields": [
                        {
                            "name": "id",
                            "field_type": "INTEGER",
                            "primary_key": True,
                            "nullable": False,
                            "auto_increment": True,
                        },
                        {
                            "name": "email",
                            "field_type": "VARCHAR(255)",
                            "nullable": False,
                            "unique": True,
                        },
                        {
                            "name": "name",
                            "field_type": "VARCHAR(100)",
                            "nullable": True,
                        },
                    ],
                    "indexes": [],
                },
            }
        ]

        filepath = generate_migration_file(
            operations, migrations_dir=tmp_path, name="create_users"
        )

        assert filepath.exists()
        assert filepath.name == "0001_create_users.py"

        content = filepath.read_text()
        assert "def upgrade(ctx):" in content
        assert "def downgrade(ctx):" in content
        assert "ctx.create_table(" in content
        assert '"users"' in content
        assert 'ctx.drop_table("users")' in content
        # First migration should have depends_on = None
        assert "depends_on = None" in content

    def test_generate_add_column_migration(self, tmp_path: Path):
        """Test generating migration for add_column operation."""
        from oxyde.migrations.generator import generate_migration_file

        operations = [
            {
                "type": "add_column",
                "table": "users",
                "field": {"name": "age", "field_type": "INTEGER", "nullable": True},
            }
        ]

        filepath = generate_migration_file(
            operations, migrations_dir=tmp_path, name="add_age_to_users"
        )

        content = filepath.read_text()
        assert 'ctx.add_column("users"' in content
        assert 'ctx.drop_column("users", "age")' in content

    def test_generate_drop_table_migration_with_definition(self, tmp_path: Path):
        """Test generating migration for drop_table with full table definition."""
        from oxyde.migrations.generator import generate_migration_file

        operations = [
            {
                "type": "drop_table",
                "name": "old_table",
                "table": {
                    "name": "old_table",
                    "fields": [
                        {
                            "name": "id",
                            "field_type": "INTEGER",
                            "primary_key": True,
                            "nullable": False,
                        },
                        {"name": "value", "field_type": "TEXT", "nullable": True},
                    ],
                    "indexes": [],
                },
            }
        ]

        filepath = generate_migration_file(
            operations, migrations_dir=tmp_path, name="drop_old_table"
        )

        content = filepath.read_text()
        # Upgrade should drop the table
        assert 'ctx.drop_table("old_table")' in content
        # Downgrade should recreate it with full definition
        assert "ctx.create_table(" in content
        assert '"old_table"' in content

    def test_generate_sequential_migrations_with_dependencies(self, tmp_path: Path):
        """Test that sequential migrations reference previous migration."""
        from oxyde.migrations.generator import generate_migration_file

        # First migration
        ops1 = [
            {
                "type": "create_table",
                "table": {"name": "t1", "fields": [], "indexes": []},
            }
        ]
        filepath1 = generate_migration_file(
            ops1, migrations_dir=tmp_path, name="create_t1"
        )

        # Second migration
        ops2 = [
            {
                "type": "create_table",
                "table": {"name": "t2", "fields": [], "indexes": []},
            }
        ]
        filepath2 = generate_migration_file(
            ops2, migrations_dir=tmp_path, name="create_t2"
        )

        content1 = filepath1.read_text()
        content2 = filepath2.read_text()

        # First migration has no dependency
        assert "depends_on = None" in content1
        # Second migration depends on first
        assert 'depends_on = "0001_create_t1"' in content2

    def test_generate_create_index_migration(self, tmp_path: Path):
        """Test generating migration for create_index operation."""
        from oxyde.migrations.generator import generate_migration_file

        operations = [
            {
                "type": "create_index",
                "table": "users",
                "index": {
                    "name": "idx_users_email",
                    "columns": ["email"],
                    "unique": True,
                },
            }
        ]

        filepath = generate_migration_file(
            operations, migrations_dir=tmp_path, name="add_email_index"
        )

        content = filepath.read_text()
        assert 'ctx.create_index("users"' in content
        assert 'ctx.drop_index("users", "idx_users_email")' in content

    def test_generate_foreign_key_migration(self, tmp_path: Path):
        """Test generating migration for add_foreign_key operation."""
        from oxyde.migrations.generator import generate_migration_file

        operations = [
            {
                "type": "add_foreign_key",
                "table": "posts",
                "fk": {
                    "name": "fk_posts_author",
                    "columns": ["author_id"],
                    "ref_table": "users",
                    "ref_columns": ["id"],
                    "on_delete": "CASCADE",
                    "on_update": "NO ACTION",
                },
            }
        ]

        filepath = generate_migration_file(
            operations, migrations_dir=tmp_path, name="add_author_fk"
        )

        content = filepath.read_text()
        assert "ctx.add_foreign_key(" in content
        assert '"fk_posts_author"' in content
        assert 'on_delete="CASCADE"' in content
        assert 'ctx.drop_foreign_key("posts", "fk_posts_author")' in content


# =============================================================================
# Replay Tests
# =============================================================================


class TestMigrationReplay:
    """Test migration replay and schema state building."""

    def test_replay_create_table(self, tmp_path: Path):
        """Test replaying create_table operation."""
        from oxyde.migrations.replay import SchemaState

        state = SchemaState()
        state.apply_operation(
            {
                "type": "create_table",
                "table": {
                    "name": "users",
                    "fields": [
                        {"name": "id", "field_type": "INTEGER", "primary_key": True},
                        {"name": "name", "field_type": "TEXT"},
                    ],
                    "indexes": [],
                },
            }
        )

        assert "users" in state.tables
        assert len(state.tables["users"]["fields"]) == 2

    def test_replay_drop_table(self):
        """Test replaying drop_table operation."""
        from oxyde.migrations.replay import SchemaState

        state = SchemaState()
        state.tables["old_table"] = {"name": "old_table", "fields": [], "indexes": []}

        state.apply_operation({"type": "drop_table", "name": "old_table"})

        assert "old_table" not in state.tables

    def test_replay_add_column(self):
        """Test replaying add_column operation."""
        from oxyde.migrations.replay import SchemaState

        state = SchemaState()
        state.tables["users"] = {
            "name": "users",
            "fields": [{"name": "id", "field_type": "INTEGER"}],
            "indexes": [],
        }

        state.apply_operation(
            {
                "type": "add_column",
                "table": "users",
                "field": {"name": "email", "field_type": "TEXT"},
            }
        )

        field_names = [f["name"] for f in state.tables["users"]["fields"]]
        assert "email" in field_names

    def test_replay_drop_column(self):
        """Test replaying drop_column operation."""
        from oxyde.migrations.replay import SchemaState

        state = SchemaState()
        state.tables["users"] = {
            "name": "users",
            "fields": [
                {"name": "id", "field_type": "INTEGER"},
                {"name": "legacy", "field_type": "TEXT"},
            ],
            "indexes": [],
        }

        state.apply_operation(
            {
                "type": "drop_column",
                "table": "users",
                "field": "legacy",
            }
        )

        field_names = [f["name"] for f in state.tables["users"]["fields"]]
        assert "legacy" not in field_names
        assert "id" in field_names

    def test_replay_rename_table(self):
        """Test replaying rename_table operation."""
        from oxyde.migrations.replay import SchemaState

        state = SchemaState()
        state.tables["old_name"] = {"name": "old_name", "fields": [], "indexes": []}

        state.apply_operation(
            {
                "type": "rename_table",
                "old_name": "old_name",
                "new_name": "new_name",
            }
        )

        assert "old_name" not in state.tables
        assert "new_name" in state.tables
        assert state.tables["new_name"]["name"] == "new_name"

    def test_replay_rename_column(self):
        """Test replaying rename_column operation."""
        from oxyde.migrations.replay import SchemaState

        state = SchemaState()
        state.tables["users"] = {
            "name": "users",
            "fields": [{"name": "old_col", "field_type": "TEXT"}],
            "indexes": [],
        }

        state.apply_operation(
            {
                "type": "rename_column",
                "table": "users",
                "old_name": "old_col",
                "new_name": "new_col",
            }
        )

        field_names = [f["name"] for f in state.tables["users"]["fields"]]
        assert "new_col" in field_names
        assert "old_col" not in field_names

    def test_replay_create_index(self):
        """Test replaying create_index operation."""
        from oxyde.migrations.replay import SchemaState

        state = SchemaState()
        state.tables["users"] = {"name": "users", "fields": [], "indexes": []}

        state.apply_operation(
            {
                "type": "create_index",
                "table": "users",
                "index": {"name": "idx_email", "columns": ["email"], "unique": True},
            }
        )

        assert len(state.tables["users"]["indexes"]) == 1
        assert state.tables["users"]["indexes"][0]["name"] == "idx_email"

    def test_replay_drop_index(self):
        """Test replaying drop_index operation."""
        from oxyde.migrations.replay import SchemaState

        state = SchemaState()
        state.tables["users"] = {
            "name": "users",
            "fields": [],
            "indexes": [{"name": "idx_email", "columns": ["email"]}],
        }

        state.apply_operation(
            {
                "type": "drop_index",
                "table": "users",
                "name": "idx_email",
            }
        )

        assert len(state.tables["users"]["indexes"]) == 0

    def test_to_snapshot(self):
        """Test converting state to snapshot format."""
        from oxyde.migrations.replay import SchemaState

        state = SchemaState()
        state.tables["users"] = {
            "name": "users",
            "fields": [{"name": "id", "field_type": "INTEGER"}],
            "indexes": [],
        }

        snapshot = state.to_snapshot()

        assert snapshot["version"] == 1
        assert "users" in snapshot["tables"]


# =============================================================================
# Dependency Resolution Tests
# =============================================================================


class TestDependencyResolution:
    """Test migration dependency resolution and ordering."""

    def test_topological_sort_linear(self, tmp_path: Path):
        """Test topological sort for linear dependencies."""
        from oxyde.migrations.replay import _topological_sort_migrations

        # Create migration files
        mig1 = tmp_path / "0001_first.py"
        mig1.write_text(
            "depends_on = None\ndef upgrade(ctx): pass\ndef downgrade(ctx): pass"
        )

        mig2 = tmp_path / "0002_second.py"
        mig2.write_text(
            'depends_on = "0001_first"\ndef upgrade(ctx): pass\ndef downgrade(ctx): pass'
        )

        mig3 = tmp_path / "0003_third.py"
        mig3.write_text(
            'depends_on = "0002_second"\ndef upgrade(ctx): pass\ndef downgrade(ctx): pass'
        )

        files = sorted(tmp_path.glob("[0-9]*.py"))
        sorted_files = _topological_sort_migrations(files)

        names = [f.stem for f in sorted_files]
        assert names == ["0001_first", "0002_second", "0003_third"]

    def test_topological_sort_no_dependencies(self, tmp_path: Path):
        """Test topological sort when no dependencies (fall back to alphabetical)."""
        from oxyde.migrations.replay import _topological_sort_migrations

        mig1 = tmp_path / "0001_a.py"
        mig1.write_text(
            "depends_on = None\ndef upgrade(ctx): pass\ndef downgrade(ctx): pass"
        )

        mig2 = tmp_path / "0002_b.py"
        mig2.write_text(
            "depends_on = None\ndef upgrade(ctx): pass\ndef downgrade(ctx): pass"
        )

        files = sorted(tmp_path.glob("[0-9]*.py"))
        sorted_files = _topological_sort_migrations(files)

        # Should be sorted alphabetically when no deps
        names = [f.stem for f in sorted_files]
        assert "0001_a" in names
        assert "0002_b" in names

    def test_topological_sort_circular_dependency(self, tmp_path: Path):
        """Test that circular dependencies raise error."""
        from oxyde.migrations.replay import _topological_sort_migrations

        mig1 = tmp_path / "0001_a.py"
        mig1.write_text(
            'depends_on = "0002_b"\ndef upgrade(ctx): pass\ndef downgrade(ctx): pass'
        )

        mig2 = tmp_path / "0002_b.py"
        mig2.write_text(
            'depends_on = "0001_a"\ndef upgrade(ctx): pass\ndef downgrade(ctx): pass'
        )

        files = sorted(tmp_path.glob("[0-9]*.py"))

        with pytest.raises(ValueError, match="Circular dependency"):
            _topological_sort_migrations(files)

    def test_get_migration_order(self, tmp_path: Path):
        """Test get_migration_order function."""
        from oxyde.migrations.replay import get_migration_order

        mig1 = tmp_path / "0001_init.py"
        mig1.write_text(
            "depends_on = None\ndef upgrade(ctx): pass\ndef downgrade(ctx): pass"
        )

        mig2 = tmp_path / "0002_users.py"
        mig2.write_text(
            'depends_on = "0001_init"\ndef upgrade(ctx): pass\ndef downgrade(ctx): pass'
        )

        order = get_migration_order(str(tmp_path))

        assert order == ["0001_init", "0002_users"]

    def test_replay_migrations_with_dependencies(self, tmp_path: Path):
        """Test replay_migrations respects dependency order."""
        from oxyde.migrations.replay import replay_migrations

        # Migration 1: create users table
        mig1 = tmp_path / "0001_users.py"
        mig1.write_text(
            """
depends_on = None

def upgrade(ctx):
    ctx.create_table("users", fields=[
        {"name": "id", "field_type": "INTEGER", "primary_key": True},
    ])

def downgrade(ctx):
    ctx.drop_table("users")
"""
        )

        # Migration 2: add email field (depends on users table existing)
        mig2 = tmp_path / "0002_email.py"
        mig2.write_text(
            """
depends_on = "0001_users"

def upgrade(ctx):
    ctx.add_column("users", {"name": "email", "field_type": "TEXT"})

def downgrade(ctx):
    ctx.drop_column("users", "email")
"""
        )

        snapshot = replay_migrations(str(tmp_path))

        # Should have users table with both id and email
        assert "users" in snapshot["tables"]
        field_names = [f["name"] for f in snapshot["tables"]["users"]["fields"]]
        assert "id" in field_names
        assert "email" in field_names


# =============================================================================
# Context Tests
# =============================================================================


class TestMigrationContext:
    """Test MigrationContext operations."""

    def test_collect_mode_create_table(self):
        """Test collecting create_table operation."""
        from oxyde.migrations.context import MigrationContext

        ctx = MigrationContext(mode="collect")
        ctx.create_table(
            "users",
            fields=[
                {"name": "id", "field_type": "INTEGER", "primary_key": True},
            ],
        )

        ops = ctx.get_collected_operations()
        assert len(ops) == 1
        assert ops[0]["type"] == "create_table"
        assert ops[0]["table"]["name"] == "users"

    def test_collect_mode_add_column(self):
        """Test collecting add_column operation."""
        from oxyde.migrations.context import MigrationContext

        ctx = MigrationContext(mode="collect")
        ctx.add_column("users", {"name": "email", "field_type": "TEXT"})

        ops = ctx.get_collected_operations()
        assert len(ops) == 1
        assert ops[0]["type"] == "add_column"
        assert ops[0]["table"] == "users"

    def test_collect_mode_drop_table(self):
        """Test collecting drop_table operation."""
        from oxyde.migrations.context import MigrationContext

        ctx = MigrationContext(mode="collect")
        ctx.drop_table("old_table")

        ops = ctx.get_collected_operations()
        assert len(ops) == 1
        assert ops[0]["type"] == "drop_table"
        assert ops[0]["name"] == "old_table"

    def test_collect_mode_multiple_operations(self):
        """Test collecting multiple operations."""
        from oxyde.migrations.context import MigrationContext

        ctx = MigrationContext(mode="collect")
        ctx.create_table("users", fields=[])
        ctx.add_column("users", {"name": "email", "field_type": "TEXT"})
        ctx.create_index("users", {"name": "idx_email", "columns": ["email"]})

        ops = ctx.get_collected_operations()
        assert len(ops) == 3
        assert ops[0]["type"] == "create_table"
        assert ops[1]["type"] == "add_column"
        assert ops[2]["type"] == "create_index"

    def test_collect_mode_foreign_key(self):
        """Test collecting foreign key operations."""
        from oxyde.migrations.context import MigrationContext

        ctx = MigrationContext(mode="collect")
        ctx.add_foreign_key(
            "posts",
            "fk_author",
            ["author_id"],
            "users",
            ["id"],
            on_delete="CASCADE",
        )

        ops = ctx.get_collected_operations()
        assert len(ops) == 1
        assert ops[0]["type"] == "add_foreign_key"
        assert ops[0]["fk"]["on_delete"] == "CASCADE"

    def test_collect_mode_check_constraint(self):
        """Test collecting check constraint operations."""
        from oxyde.migrations.context import MigrationContext

        ctx = MigrationContext(mode="collect")
        ctx.add_check("users", "chk_age", "age >= 0")

        ops = ctx.get_collected_operations()
        assert len(ops) == 1
        assert ops[0]["type"] == "add_check"
        assert ops[0]["check"]["expression"] == "age >= 0"

    def test_dialect_property(self):
        """Test dialect property."""
        from oxyde.migrations.context import MigrationContext

        ctx_sqlite = MigrationContext(mode="collect", dialect="sqlite")
        ctx_postgres = MigrationContext(mode="collect", dialect="postgres")

        assert ctx_sqlite.dialect == "sqlite"
        assert ctx_postgres.dialect == "postgres"


# =============================================================================
# Tracker Tests
# =============================================================================


class TestMigrationTracker:
    """Test migration file tracking utilities."""

    def test_get_migration_files_empty(self, tmp_path: Path):
        """Test getting migration files from empty directory."""
        from oxyde.migrations.tracker import get_migration_files

        files = get_migration_files(str(tmp_path))
        assert files == []

    def test_get_migration_files_sorted(self, tmp_path: Path):
        """Test that migration files are sorted by number."""
        from oxyde.migrations.tracker import get_migration_files

        (tmp_path / "0003_third.py").write_text("")
        (tmp_path / "0001_first.py").write_text("")
        (tmp_path / "0002_second.py").write_text("")

        files = get_migration_files(str(tmp_path))
        names = [f.name for f in files]

        assert names == ["0001_first.py", "0002_second.py", "0003_third.py"]

    def test_get_migration_files_ignores_non_migrations(self, tmp_path: Path):
        """Test that non-migration files are ignored."""
        from oxyde.migrations.tracker import get_migration_files

        (tmp_path / "0001_migration.py").write_text("")
        (tmp_path / "__init__.py").write_text("")
        (tmp_path / "utils.py").write_text("")
        (tmp_path / "README.md").write_text("")

        files = get_migration_files(str(tmp_path))

        assert len(files) == 1
        assert files[0].name == "0001_migration.py"

    def test_get_pending_migrations(self, tmp_path: Path):
        """Test getting pending migrations."""
        from oxyde.migrations.tracker import get_pending_migrations

        (tmp_path / "0001_first.py").write_text("")
        (tmp_path / "0002_second.py").write_text("")
        (tmp_path / "0003_third.py").write_text("")

        applied = ["0001_first"]
        pending = get_pending_migrations(str(tmp_path), applied)

        names = [f.stem for f in pending]
        assert "0001_first" not in names
        assert "0002_second" in names
        assert "0003_third" in names

    def test_get_pending_migrations_all_applied(self, tmp_path: Path):
        """Test when all migrations are applied."""
        from oxyde.migrations.tracker import get_pending_migrations

        (tmp_path / "0001_first.py").write_text("")
        (tmp_path / "0002_second.py").write_text("")

        applied = ["0001_first", "0002_second"]
        pending = get_pending_migrations(str(tmp_path), applied)

        assert pending == []


# =============================================================================
# Executor Dependency Tests (without DB)
# =============================================================================


class TestExecutorDependencyChecks:
    """Test executor dependency checking logic."""

    def test_check_migration_dependency_satisfied(self, tmp_path: Path):
        """Test dependency check passes when dependency is satisfied."""
        from oxyde.migrations.executor import _check_migration_dependency

        mig = tmp_path / "0002_second.py"
        mig.write_text('depends_on = "0001_first"\ndef upgrade(ctx): pass')

        applied = {"0001_first"}

        # Should not raise
        _check_migration_dependency(mig, applied)

    def test_check_migration_dependency_not_satisfied(self, tmp_path: Path):
        """Test dependency check fails when dependency not satisfied."""
        from oxyde.migrations.executor import _check_migration_dependency

        mig = tmp_path / "0002_second.py"
        mig.write_text('depends_on = "0001_first"\ndef upgrade(ctx): pass')

        applied = set()  # 0001_first not applied

        with pytest.raises(RuntimeError, match="depends on"):
            _check_migration_dependency(mig, applied)

    def test_check_migration_dependency_none(self, tmp_path: Path):
        """Test dependency check passes when no dependency."""
        from oxyde.migrations.executor import _check_migration_dependency

        mig = tmp_path / "0001_first.py"
        mig.write_text("depends_on = None\ndef upgrade(ctx): pass")

        applied = set()

        # Should not raise
        _check_migration_dependency(mig, applied)

    def test_check_rollback_dependency_safe(self, tmp_path: Path):
        """Test rollback check passes when no migration depends on target."""
        from oxyde.migrations.executor import _check_rollback_dependency

        mig1 = tmp_path / "0001_first.py"
        mig1.write_text("depends_on = None\ndef upgrade(ctx): pass")

        mig2 = tmp_path / "0002_second.py"
        mig2.write_text('depends_on = "0001_first"\ndef upgrade(ctx): pass')

        # Rolling back 0002 is safe (nothing depends on it)
        applied = ["0001_first", "0002_second"]
        _check_rollback_dependency("0002_second", str(tmp_path), applied)

    def test_check_rollback_dependency_blocked(self, tmp_path: Path):
        """Test rollback check fails when another migration depends on target."""
        from oxyde.migrations.executor import _check_rollback_dependency

        mig1 = tmp_path / "0001_first.py"
        mig1.write_text("depends_on = None\ndef upgrade(ctx): pass")

        mig2 = tmp_path / "0002_second.py"
        mig2.write_text('depends_on = "0001_first"\ndef upgrade(ctx): pass')

        # Rolling back 0001 should fail because 0002 depends on it
        applied = ["0001_first", "0002_second"]

        with pytest.raises(RuntimeError, match="depends on it"):
            _check_rollback_dependency("0001_first", str(tmp_path), applied)


# =============================================================================
# Drop Operation Reversibility Tests
# =============================================================================


class TestDropOperationReversibility:
    """Test that drop operations can be properly reversed."""

    def test_drop_column_with_definition_generates_add_column(self, tmp_path: Path):
        """Test drop_column with field_def generates add_column in downgrade."""
        from oxyde.migrations.generator import generate_migration_file

        operations = [
            {
                "type": "drop_column",
                "table": "users",
                "field": "legacy_field",
                "field_def": {
                    "name": "legacy_field",
                    "field_type": "VARCHAR(100)",
                    "nullable": True,
                    "default": "'unknown'",
                },
            }
        ]

        filepath = generate_migration_file(
            operations, migrations_dir=tmp_path, name="drop_legacy"
        )

        content = filepath.read_text()
        # Upgrade drops the column
        assert 'ctx.drop_column("users", "legacy_field")' in content
        # Downgrade should add it back with full definition
        assert 'ctx.add_column("users"' in content
        assert "legacy_field" in content

    def test_drop_index_with_definition_generates_create_index(self, tmp_path: Path):
        """Test drop_index with index_def generates create_index in downgrade."""
        from oxyde.migrations.generator import generate_migration_file

        operations = [
            {
                "type": "drop_index",
                "table": "users",
                "name": "idx_email",
                "index_def": {
                    "name": "idx_email",
                    "columns": ["email"],
                    "unique": True,
                },
            }
        ]

        filepath = generate_migration_file(
            operations, migrations_dir=tmp_path, name="drop_index"
        )

        content = filepath.read_text()
        assert 'ctx.drop_index("users", "idx_email")' in content
        assert 'ctx.create_index("users"' in content

    def test_drop_foreign_key_with_definition_generates_add_fk(self, tmp_path: Path):
        """Test drop_foreign_key with fk_def generates add_foreign_key in downgrade."""
        from oxyde.migrations.generator import generate_migration_file

        operations = [
            {
                "type": "drop_foreign_key",
                "table": "posts",
                "name": "fk_author",
                "fk_def": {
                    "name": "fk_author",
                    "columns": ["author_id"],
                    "ref_table": "users",
                    "ref_columns": ["id"],
                    "on_delete": "CASCADE",
                    "on_update": "NO ACTION",
                },
            }
        ]

        filepath = generate_migration_file(
            operations, migrations_dir=tmp_path, name="drop_fk"
        )

        content = filepath.read_text()
        assert 'ctx.drop_foreign_key("posts", "fk_author")' in content
        assert "ctx.add_foreign_key(" in content
        assert 'on_delete="CASCADE"' in content

    def test_drop_check_with_definition_generates_add_check(self, tmp_path: Path):
        """Test drop_check with check_def generates add_check in downgrade."""
        from oxyde.migrations.generator import generate_migration_file

        operations = [
            {
                "type": "drop_check",
                "table": "users",
                "name": "chk_age",
                "check_def": {
                    "name": "chk_age",
                    "expression": "age >= 0",
                },
            }
        ]

        filepath = generate_migration_file(
            operations, migrations_dir=tmp_path, name="drop_check"
        )

        content = filepath.read_text()
        assert 'ctx.drop_check("users", "chk_age")' in content
        assert 'ctx.add_check("users", "chk_age", "age >= 0")' in content

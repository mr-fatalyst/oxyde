"""Tests for `oxyde migrations squash` core logic.

Covers: legacy-format history → single new-format initial migration,
raw-SQL detection, history replacement, idempotency, empty dir.
"""

from __future__ import annotations

from pathlib import Path

from oxyde.migrations.replay import replay_migrations
from oxyde.migrations.squash import squash_migrations

LEGACY_0001 = '''"""Legacy-format migration (python_type field dicts)."""

depends_on = None


def upgrade(ctx):
    ctx.create_table(
        "users",
        fields=[
            {
                "name": "id",
                "python_type": "int",
                "db_type": None,
                "nullable": False,
                "primary_key": True,
                "unique": False,
                "default": None,
                "auto_increment": True,
            },
            {
                "name": "email",
                "python_type": "str",
                "db_type": None,
                "nullable": False,
                "primary_key": False,
                "unique": True,
                "default": None,
                "auto_increment": False,
                "max_length": 255,
            },
        ],
    )


def downgrade(ctx):
    ctx.drop_table("users")
'''

LEGACY_0002 = '''"""Legacy add_column + manual SQL."""

depends_on = "0001_users"


def upgrade(ctx):
    ctx.add_column(
        "users",
        {
            "name": "age",
            "python_type": "int",
            "db_type": None,
            "nullable": True,
            "primary_key": False,
            "unique": False,
            "default": None,
            "auto_increment": False,
        },
    )
    ctx.execute("UPDATE users SET age = 0 WHERE age IS NULL")


def downgrade(ctx):
    ctx.drop_column("users", "age")
'''


def _write_legacy_history(migrations_dir: Path) -> None:
    migrations_dir.mkdir(parents=True, exist_ok=True)
    (migrations_dir / "0001_users.py").write_text(LEGACY_0001)
    (migrations_dir / "0002_add_age.py").write_text(LEGACY_0002)


class TestSquash:
    def test_empty_dir_is_noop(self, tmp_path):
        result = squash_migrations(tmp_path)
        assert result.new_file is None
        assert result.deleted_files == []

    def test_replaces_history_with_single_file(self, tmp_path, recwarn):
        _write_legacy_history(tmp_path)

        result = squash_migrations(tmp_path, name="squashed")

        assert result.new_file is not None
        assert result.new_file.name == "0001_squashed.py"
        assert sorted(result.deleted_files) == ["0001_users.py", "0002_add_age.py"]
        # Only the new file remains
        remaining = sorted(p.name for p in tmp_path.glob("[0-9]*.py"))
        assert remaining == ["0001_squashed.py"]
        assert result.table_count == 1

    def test_new_file_is_in_spec_format(self, tmp_path):
        _write_legacy_history(tmp_path)
        result = squash_migrations(tmp_path)

        content = result.new_file.read_text()
        assert "column_type" in content
        assert "python_type" not in content
        # depends_on resets — this is the new initial migration
        assert "depends_on = None" in content

    def test_raw_sql_files_reported(self, tmp_path):
        _write_legacy_history(tmp_path)
        result = squash_migrations(tmp_path)

        assert result.raw_sql_files == ["0002_add_age.py"]
        assert sorted(result.legacy_files) == ["0001_users.py", "0002_add_age.py"]

    def test_squashed_history_replays_to_same_schema(self, tmp_path, recwarn):
        _write_legacy_history(tmp_path)
        before = replay_migrations(str(tmp_path))

        squash_migrations(tmp_path)
        after = replay_migrations(str(tmp_path))

        assert before["tables"].keys() == after["tables"].keys()
        users_before = {f["name"]: f for f in before["tables"]["users"]["fields"]}
        users_after = {f["name"]: f for f in after["tables"]["users"]["fields"]}
        assert users_before.keys() == users_after.keys()
        for col in users_before:
            assert (
                users_before[col]["column_type"] == users_after[col]["column_type"]
            ), col

    def test_squash_is_idempotent(self, tmp_path):
        _write_legacy_history(tmp_path)
        squash_migrations(tmp_path)
        first = replay_migrations(str(tmp_path))

        result = squash_migrations(tmp_path)
        second = replay_migrations(str(tmp_path))

        # Re-squashing the squashed history: no legacy, no raw SQL, same schema
        assert result.legacy_files == []
        assert result.raw_sql_files == []
        assert first["tables"] == second["tables"]

"""End-to-end migration pipeline tests (without a real database).

Flow per test:
    Model(before) → extract_current_schema → old_snapshot
    Model(after)  → extract_current_schema → new_snapshot
    migration_compute_diff → operations
    generate_migration_file → on-disk .py
    load_migration_module + ctx.upgrade(collect) → ops replayed
    migration_to_sql → SQL for each dialect

Catches composition bugs that unit layers miss — e.g. the drop_fk bug
where Python/Rust payload shapes diverged silently.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from oxyde import Field, Model
from oxyde.core import migration_compute_diff, migration_to_sql
from oxyde.migrations.context import MigrationContext
from oxyde.migrations.extract import extract_current_schema
from oxyde.migrations.generator import generate_migration_file
from oxyde.migrations.utils import load_migration_module
from oxyde.models.registry import clear_registry


ALL_DIALECTS = ["postgres", "mysql", "sqlite"]
NON_SQLITE = ["postgres", "mysql"]
PARTIAL_INDEX_DIALECTS = ["postgres", "sqlite"]


def _snapshot_from_models(models: list[type[Model]], dialect: str) -> dict:
    """Register given models fresh and return their schema snapshot."""
    clear_registry()
    for model in models:
        from oxyde.models.registry import register_table

        register_table(model, overwrite=True)
    return extract_current_schema(dialect=dialect)


def _run_pipeline(
    old_models: list[type[Model]],
    new_models: list[type[Model]],
    dialect: str,
    tmp_path: Path,
    name: str,
) -> tuple[list[dict], list[str], list[str]]:
    """Run full pipeline and return (ops, upgrade_sql, downgrade_sql)."""
    old = _snapshot_from_models(old_models, dialect)
    new = _snapshot_from_models(new_models, dialect)

    ops_json = migration_compute_diff(json.dumps(old), json.dumps(new))
    ops = json.loads(ops_json)
    assert ops, "diff produced no operations — test setup is wrong"

    filepath = generate_migration_file(ops, migrations_dir=tmp_path, name=name)
    module = load_migration_module(filepath)
    assert module is not None

    up_ctx = MigrationContext(mode="collect", dialect=dialect)
    module.upgrade(up_ctx)
    up_sql = migration_to_sql(
        json.dumps(up_ctx.get_collected_operations()), dialect
    )

    down_ctx = MigrationContext(mode="collect", dialect=dialect)
    module.downgrade(down_ctx)
    down_ops = down_ctx.get_collected_operations()
    # downgrade may contain a TODO comment instead of ops — only render if non-empty
    down_sql = (
        migration_to_sql(json.dumps(down_ops), dialect) if down_ops else []
    )

    return ops, up_sql, down_sql


class TestCreateTablePipeline:
    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_create_table_from_scratch(self, tmp_path, dialect):
        class User(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str
            active: bool = True

            class Meta:
                is_table = True
                table_name = "users"

        ops, up_sql, down_sql = _run_pipeline(
            [], [User], dialect, tmp_path, "create_users"
        )

        assert any(op["type"] == "create_table" for op in ops)
        assert any("CREATE TABLE" in s.upper() and "users" in s for s in up_sql)
        assert any("DROP TABLE" in s.upper() and "users" in s for s in down_sql)


class TestAddColumnPipeline:
    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_add_column(self, tmp_path, dialect):
        class UserV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str

            class Meta:
                is_table = True
                table_name = "users"

        class UserV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str
            nickname: str | None = Field(default=None, db_nullable=True)

            class Meta:
                is_table = True
                table_name = "users"

        ops, up_sql, down_sql = _run_pipeline(
            [UserV1], [UserV2], dialect, tmp_path, "add_nickname"
        )

        assert any(op["type"] == "add_column" for op in ops)
        assert any("ADD" in s.upper() and "nickname" in s for s in up_sql)
        assert any("nickname" in s for s in down_sql)


class TestDropColumnPipeline:
    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_drop_column(self, tmp_path, dialect):
        class UserV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str
            deprecated: str | None = Field(default=None, db_nullable=True)

            class Meta:
                is_table = True
                table_name = "users"

        class UserV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str

            class Meta:
                is_table = True
                table_name = "users"

        ops, up_sql, down_sql = _run_pipeline(
            [UserV1], [UserV2], dialect, tmp_path, "drop_deprecated"
        )

        assert any(op["type"] == "drop_column" for op in ops)
        assert any("DROP" in s.upper() and "deprecated" in s for s in up_sql)
        # Reverse must re-add the column (field_def persisted in downgrade)
        assert any("ADD" in s.upper() and "deprecated" in s for s in down_sql)


class TestDropForeignKeyPipeline:
    """Exact scenario from the bug report.

    Model Book(author: Author) → Book(author_id: int) keeps the column
    but removes the FK relationship; makemigrations + sqlmigrate must succeed.
    """

    @pytest.mark.parametrize("dialect", NON_SQLITE)
    def test_drop_fk_keeping_column(self, tmp_path, dialect):
        class AuthorV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True
                table_name = "authors"

        class BookV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            title: str
            author: AuthorV1 | None = Field(default=None, db_on_delete="CASCADE")

            class Meta:
                is_table = True
                table_name = "books"

        class AuthorV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            name: str

            class Meta:
                is_table = True
                table_name = "authors"

        class BookV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            title: str
            author_id: int | None = Field(default=None, db_nullable=True)

            class Meta:
                is_table = True
                table_name = "books"

        ops, up_sql, down_sql = _run_pipeline(
            [AuthorV1, BookV1],
            [AuthorV2, BookV2],
            dialect,
            tmp_path,
            "drop_author_fk",
        )

        assert any(op["type"] == "drop_foreign_key" for op in ops)
        assert any(
            "DROP" in s.upper() and "fk_books_author_id" in s for s in up_sql
        )
        # Reverse must re-add the FK, not just mention its name
        assert any(
            "ADD" in s.upper() and "fk_books_author_id" in s for s in down_sql
        )


class TestIndexPipeline:
    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_create_index(self, tmp_path, dialect):
        from oxyde import Index

        class UserV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str

            class Meta:
                is_table = True
                table_name = "users"

        class UserV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str

            class Meta:
                is_table = True
                table_name = "users"
                indexes = [Index(fields=["email"], name="idx_users_email")]

        ops, up_sql, down_sql = _run_pipeline(
            [UserV1], [UserV2], dialect, tmp_path, "add_email_index"
        )

        assert any(op["type"] == "create_index" for op in ops)
        assert any(
            "CREATE" in s.upper() and "idx_users_email" in s for s in up_sql
        )
        assert any("idx_users_email" in s for s in down_sql)

    @pytest.mark.parametrize("dialect", PARTIAL_INDEX_DIALECTS)
    def test_create_partial_index_preserves_where(self, tmp_path, dialect):
        from oxyde import Index

        class UserV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str
            deleted_at: str | None = Field(default=None)

            class Meta:
                is_table = True
                table_name = "users"

        class UserV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str
            deleted_at: str | None = Field(default=None)

            class Meta:
                is_table = True
                table_name = "users"
                indexes = [
                    Index(
                        fields=["email"],
                        name="idx_users_active_email",
                        unique=True,
                        where="  deleted_at IS NULL  ",
                    )
                ]

        ops, up_sql, down_sql = _run_pipeline(
            [UserV1], [UserV2], dialect, tmp_path, "add_active_email_index"
        )

        create_ops = [op for op in ops if op["type"] == "create_index"]
        assert create_ops[0]["index"]["where"] == "deleted_at IS NULL"
        assert any("WHERE deleted_at IS NULL" in s for s in up_sql)
        assert any("idx_users_active_email" in s for s in down_sql)

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_drop_index(self, tmp_path, dialect):
        from oxyde import Index

        class UserV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str

            class Meta:
                is_table = True
                table_name = "users"
                indexes = [Index(fields=["email"], name="idx_users_email")]

        class UserV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            email: str

            class Meta:
                is_table = True
                table_name = "users"

        ops, up_sql, down_sql = _run_pipeline(
            [UserV1], [UserV2], dialect, tmp_path, "drop_email_index"
        )

        assert any(op["type"] == "drop_index" for op in ops)
        assert any("DROP" in s.upper() and "idx_users_email" in s for s in up_sql)
        # Reverse re-creates index
        assert any(
            "CREATE" in s.upper() and "idx_users_email" in s for s in down_sql
        )


class TestCyclicForeignKeys:
    """Cycles only matter when the affected tables are actually being created
    or dropped. For unchanged tables the topo-sort is irrelevant. The diff
    planner must therefore restrict its topo-sort to the create/drop subset,
    not the entire snapshot.
    """

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_noop_diff_on_cyclic_schema(self, dialect):
        class Company(Model):
            id: int | None = Field(default=None, db_pk=True)
            user: User | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "companies"

        class User(Model):
            id: int | None = Field(default=None, db_pk=True)
            company: Company | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "users"

        snap = _snapshot_from_models([Company, User], dialect)
        ops_json = migration_compute_diff(json.dumps(snap), json.dumps(snap))
        ops = json.loads(ops_json)
        assert ops == [], (
            f"no-op diff on cyclic schema must produce no operations, got {ops}"
        )

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_add_column_in_cyclic_schema(self, tmp_path, dialect):
        class CompanyV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            user: UserV1 | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "companies"

        class UserV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            company: CompanyV1 | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "users"

        class CompanyV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            user: UserV2 | None = Field(default=None, db_on_delete="NO ACTION")
            name: str | None = Field(default=None, db_nullable=True)

            class Meta:
                is_table = True
                table_name = "companies"

        class UserV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            company: CompanyV2 | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "users"

        ops, up_sql, _ = _run_pipeline(
            [CompanyV1, UserV1],
            [CompanyV2, UserV2],
            dialect,
            tmp_path,
            "add_company_name",
        )

        add_columns = [op for op in ops if op["type"] == "add_column"]
        assert len(add_columns) == 1, (
            f"expected exactly one AddColumn, got {ops}"
        )
        assert add_columns[0]["table"] == "companies"
        assert add_columns[0]["field"]["name"] == "name"
        assert any(
            "ADD" in s.upper() and "name" in s.lower() for s in up_sql
        )

    @pytest.mark.parametrize("dialect", ALL_DIALECTS)
    def test_create_table_referencing_cyclic_subset(self, tmp_path, dialect):
        class CompanyV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            user: UserV1 | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "companies"

        class UserV1(Model):
            id: int | None = Field(default=None, db_pk=True)
            company: CompanyV1 | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "users"

        class CompanyV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            user: UserV2 | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "companies"

        class UserV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            company: CompanyV2 | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "users"

        class OrderV2(Model):
            id: int | None = Field(default=None, db_pk=True)
            user: UserV2 | None = Field(default=None, db_on_delete="NO ACTION")

            class Meta:
                is_table = True
                table_name = "orders"

        ops, up_sql, _ = _run_pipeline(
            [CompanyV1, UserV1],
            [CompanyV2, UserV2, OrderV2],
            dialect,
            tmp_path,
            "add_orders_table",
        )

        create_tables = [
            op["table"]["name"] for op in ops if op["type"] == "create_table"
        ]
        assert create_tables == ["orders"], (
            f"expected exactly one CreateTable(orders), got {ops}"
        )
        assert any("CREATE TABLE" in s.upper() and "orders" in s for s in up_sql)

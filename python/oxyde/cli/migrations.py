"""Migration commands (makemigrations, migrate, showmigrations, sqlmigrate)."""

import asyncio
import json
from pathlib import Path

import typer

from oxyde.cli.app import (
    app,
    ensure_migrations_dir,
    init_databases,
    load_config_or_exit,
    require_databases,
)
from oxyde.migrations.config import import_models


@app.command()
def makemigrations(
    name: str | None = typer.Option(None, help="Migration name"),
    dry_run: bool = typer.Option(
        False, help="Show what would be created without actually creating"
    ),
) -> None:
    """
    Create migration files by comparing current models with replayed migrations.

    Scans all Model subclasses, replays existing migrations,
    computes diff, and generates a new migration file if changes detected.
    """
    from oxyde.core import migration_compute_diff
    from oxyde.migrations import (
        extract_current_schema,
        generate_migration_file,
        replay_migrations,
    )

    # Load config
    config = load_config_or_exit()

    typer.echo("📝 Creating migrations...")
    typer.echo()

    # Import models
    typer.echo("0️⃣  Loading models...")
    imported = import_models(config.models)
    if imported == 0:
        typer.secho("   ❌ No modules imported", fg=typer.colors.RED)
        raise typer.Exit(1)
    typer.echo(f"   ✅ Imported {imported} module(s)")

    # Step 1: Extract current schema from models
    typer.echo()
    typer.echo("1️⃣  Extracting schema from models...")
    try:
        current_schema = extract_current_schema(dialect=config.dialect)
        table_count = len(current_schema["tables"])
        tables = ", ".join(current_schema["tables"].keys())
        if table_count > 0:
            typer.echo(f"   ✅ Found {table_count} table(s): {tables}")
        else:
            typer.secho("   ⚠️  No tables found", fg=typer.colors.YELLOW)
            typer.echo("   Make sure your models have 'class Meta: is_table = True'")
    except Exception as e:
        typer.secho(f"   ❌ Error extracting schema: {e}", fg=typer.colors.RED)
        raise typer.Exit(1)

    # Step 2: Replay existing migrations
    typer.echo()
    typer.echo("2️⃣  Replaying existing migrations...")
    migrations_path = ensure_migrations_dir(config.migrations_dir, dry_run=dry_run)

    if not migrations_path.exists():
        old_schema = {"version": 1, "tables": {}}
    else:
        try:
            old_schema = replay_migrations(config.migrations_dir)
            migration_count = len(list(migrations_path.glob("[0-9]*.py")))
            typer.echo(f"   ✅ Replayed {migration_count} migration(s)")
        except Exception as e:
            typer.secho(f"   ❌ Error replaying migrations: {e}", fg=typer.colors.RED)
            typer.echo("   Fix the broken migration(s) before running makemigrations.")
            raise typer.Exit(1)

    # Step 3: Compute diff
    typer.echo()
    typer.echo("3️⃣  Computing diff...")
    try:
        operations_json = migration_compute_diff(
            json.dumps(old_schema), json.dumps(current_schema)
        )
        operations = json.loads(operations_json)

        if not operations:
            typer.echo()
            typer.secho("   ✨ No changes detected", fg=typer.colors.GREEN)
            return

        typer.echo(f"   ✅ Found {len(operations)} operation(s):")
        for op in operations:
            op_type = op.get("type", "unknown")
            if op_type == "create_table":
                typer.echo(f"      - Create table: {op['table']['name']}")
            elif op_type == "drop_table":
                typer.echo(f"      - Drop table: {op['name']}")
            elif op_type == "add_column":
                typer.echo(f"      - Add column: {op['table']}.{op['field']['name']}")
            elif op_type == "drop_column":
                typer.echo(f"      - Drop column: {op['table']}.{op['field']}")
            elif op_type == "alter_enum_type":
                typer.secho(
                    f"      - Manual enum change: {op['name']} "
                    f"{op['old_values']} -> {op['new_values']}",
                    fg=typer.colors.YELLOW,
                )
            else:
                typer.echo(f"      - {op_type}")

        if any(op.get("type") == "alter_enum_type" for op in operations):
            typer.secho(
                "   ⚠️  One or more enum types changed in a way that requires "
                "manual SQL. The migration file will include a ctx.require_manual(...) "
                "guard; replace it with ctx.execute(...) and keep ctx.alter_enum_type(...).",
                fg=typer.colors.YELLOW,
            )

    except Exception as e:
        typer.secho(f"   ❌ Error computing diff: {e}", fg=typer.colors.RED)
        raise typer.Exit(1)

    # Step 4: Generate migration file
    typer.echo()
    if dry_run:
        typer.secho("   [DRY RUN] Would create migration file", fg=typer.colors.YELLOW)
        typer.echo(f"   Migration name: {name or 'auto'}")
        typer.echo(f"   Operations: {len(operations)}")
    else:
        typer.echo("4️⃣  Generating migration file...")
        try:
            filepath = generate_migration_file(
                operations,
                migrations_dir=config.migrations_dir,
                name=name,
            )
            typer.echo()
            typer.secho(f"   ✅ Created: {filepath}", fg=typer.colors.GREEN, bold=True)
        except Exception as e:
            typer.secho(f"   ❌ Error generating migration: {e}", fg=typer.colors.RED)
            raise typer.Exit(1)

        # Step 5: Generate type stubs
        typer.echo()
        typer.echo("5️⃣  Generating type stubs...")
        try:
            from oxyde.codegen import generate_stubs_for_models, write_stubs
            from oxyde.models.registry import registered_tables

            models = list(registered_tables().values())
            if models:
                stub_mapping = generate_stubs_for_models(models)
                write_stubs(stub_mapping)
                typer.secho(
                    f"   ✅ Generated {len(stub_mapping)} stub file(s)",
                    fg=typer.colors.GREEN,
                )
            else:
                typer.echo("   ⚠️  No models to generate stubs for")
        except Exception as e:
            typer.secho(
                f"   ⚠️  Warning: Could not generate stubs: {e}",
                fg=typer.colors.YELLOW,
            )
            # Don't fail migration on stub generation errors


@app.command()
def migrate(
    target: str | None = typer.Argument(None, help="Target migration name (e.g. 0001)"),
    fake: bool = typer.Option(
        False, help="Mark migrations as applied without running SQL"
    ),
    db_alias: str = typer.Option("default", help="Database connection alias"),
) -> None:
    """
    Apply all pending migrations.

    Runs all migrations that haven't been applied yet.
    Specify migration name to migrate to a specific version.
    """
    from oxyde.migrations import (
        apply_migrations,
        get_applied_migrations,
        get_pending_migrations,
    )

    # Load config
    config = load_config_or_exit()
    require_databases(config, db_alias)

    typer.echo("⏳ Applying migrations...")
    typer.echo()

    if fake:
        typer.secho(
            "⚠️  [FAKE MODE] Marking migrations as applied without executing SQL",
            fg=typer.colors.YELLOW,
        )
        typer.echo()

    async def run_migrate() -> tuple[str | None, list]:
        from oxyde.migrations import get_migration_files, rollback_migrations

        # Initialize database connection
        await init_databases({db_alias: config.databases[db_alias]})

        # Get current state
        applied = await get_applied_migrations(db_alias)
        all_migrations = get_migration_files(config.migrations_dir)

        # If target specified, check if we need to rollback
        if target:
            # Special case: "zero" means rollback all migrations
            if target.lower() == "zero":
                if not applied:
                    typer.secho("✨ No migrations to roll back", fg=typer.colors.GREEN)
                    return "rollback", []

                typer.echo(f"Rolling back all {len(applied)} migration(s)...")
                typer.echo()

                rolled_back = await rollback_migrations(
                    steps=len(applied),
                    migrations_dir=config.migrations_dir,
                    db_alias=db_alias,
                    fake=fake,
                )
                return "rollback", rolled_back

            # Find target migration index
            target_idx = -1
            for i, m in enumerate(all_migrations):
                if m.stem == target or m.stem.startswith(target):
                    target_idx = i
                    break

            if target_idx == -1:
                typer.secho(f"❌ Migration '{target}' not found", fg=typer.colors.RED)
                return None, []

            # Find current position (last applied migration)
            current_idx = -1
            for i, m in enumerate(all_migrations):
                if m.stem in applied:
                    current_idx = i

            # If target is before current position, rollback
            if target_idx < current_idx:
                steps = current_idx - target_idx
                typer.echo(f"Rolling back {steps} migration(s) to reach {target}...")
                typer.echo()

                rolled_back = await rollback_migrations(
                    steps=steps,
                    migrations_dir=config.migrations_dir,
                    db_alias=db_alias,
                    fake=fake,
                )
                return "rollback", rolled_back

        # Forward migration
        pending = get_pending_migrations(config.migrations_dir, applied)

        if not pending:
            typer.secho("✨ No pending migrations", fg=typer.colors.GREEN)
            return "apply", []

        # Filter pending if target specified
        if target:
            filtered = []
            for m in pending:
                filtered.append(m)
                if m.stem == target or m.stem.startswith(target):
                    break
            pending = filtered

        # Show what will be applied
        typer.echo(f"Found {len(pending)} pending migration(s):")
        for migration_path in pending:
            typer.echo(f"  - {migration_path.stem}")

        typer.echo()

        # Apply migrations
        if target:
            typer.echo(f"Migrating to: {target}")
        else:
            typer.echo("Migrating to latest...")

        applied_migrations = await apply_migrations(
            migrations_dir=config.migrations_dir,
            db_alias=db_alias,
            target=target,
            fake=fake,
        )

        return "apply", applied_migrations

    try:
        result = asyncio.run(run_migrate())
        if result is None or result[0] is None:
            raise typer.Exit(1)

        action, migrations = result

        if migrations:
            typer.echo()
            if action == "rollback":
                typer.secho(
                    f"✅ Rolled back {len(migrations)} migration(s)",
                    fg=typer.colors.GREEN,
                    bold=True,
                )
            else:
                typer.secho(
                    f"✅ Applied {len(migrations)} migration(s)",
                    fg=typer.colors.GREEN,
                    bold=True,
                )
            for name in migrations:
                typer.echo(f"   - {name}")

    except Exception as e:
        typer.secho(f"❌ Error applying migrations: {e}", fg=typer.colors.RED)
        raise typer.Exit(1)


@app.command()
def showmigrations(
    db_alias: str = typer.Option("default", help="Database connection alias"),
) -> None:
    """
    Show list of all migrations with their status (applied/pending).
    """
    from oxyde.migrations import get_applied_migrations, get_migration_files

    # Load config
    config = load_config_or_exit()
    require_databases(config, db_alias)

    typer.echo("📋 Migrations status:")
    typer.echo()

    async def run_show() -> set[str]:
        # Initialize database connection
        await init_databases({db_alias: config.databases[db_alias]})

        # Get applied migrations
        applied = await get_applied_migrations(db_alias)
        return set(applied)

    try:
        applied_set = asyncio.run(run_show())

        # Get all migration files
        all_migrations = get_migration_files(config.migrations_dir)

        if not all_migrations:
            typer.secho("No migrations found", fg=typer.colors.YELLOW)
            return

        # Show status for each migration
        for migration_path in all_migrations:
            name = migration_path.stem
            if name in applied_set:
                typer.secho(f"  [✓] {name}", fg=typer.colors.GREEN)
            else:
                typer.echo(f"  [ ] {name}")

        typer.echo()
        typer.echo(f"Total: {len(all_migrations)} migration(s)")
        typer.echo(f"Applied: {len(applied_set)}")
        typer.echo(f"Pending: {len(all_migrations) - len(applied_set)}")

    except Exception as e:
        typer.secho(f"❌ Error reading migrations: {e}", fg=typer.colors.RED)
        raise typer.Exit(1)


@app.command()
def sqlmigrate(
    name: str,
) -> None:
    """
    Print the SQL for a specific migration.

    Args:
        name: Name of the migration file (e.g., 0001_initial)
    """
    from oxyde.core import migration_to_sql
    from oxyde.migrations.context import MigrationContext
    from oxyde.migrations.utils import load_migration_module

    # Load config
    config = load_config_or_exit()

    typer.echo(f"📝 SQL for migration: {name}")
    typer.echo()

    try:
        # Find migration file
        migration_path = Path(config.migrations_dir) / f"{name}.py"
        if not migration_path.exists():
            typer.secho(f"❌ Migration not found: {name}", fg=typer.colors.RED)
            raise typer.Exit(1)

        # Import migration module
        module = load_migration_module(migration_path)

        if module is None or not hasattr(module, "upgrade"):
            typer.secho("❌ Migration missing upgrade() function", fg=typer.colors.RED)
            raise typer.Exit(1)

        # Create context in collect mode to get operations
        ctx = MigrationContext(mode="collect", dialect=config.dialect)
        module.upgrade(ctx)

        operations = ctx.get_collected_operations()

        if not operations:
            typer.echo("-- No operations (empty migration)")
            return

        # Convert operations to SQL
        operations_json = json.dumps(operations)
        sql_statements = migration_to_sql(operations_json, config.dialect)

        # Print SQL
        typer.secho(f"-- Generated SQL ({config.dialect}):", fg=typer.colors.CYAN)
        typer.echo()
        for sql in sql_statements:
            typer.echo(sql)
            typer.echo()

        # Manual ops emit no SQL — surface them so the preview is not empty.
        for op in operations:
            if op.get("type") == "alter_enum_type":
                typer.secho(
                    f"-- manual migration required: enum {op['name']} "
                    f"{op['old_values']} -> {op['new_values']} "
                    "(no automatic SQL; the migration file pairs this with "
                    "ctx.require_manual(...))",
                    fg=typer.colors.YELLOW,
                )
                typer.echo()

    except Exception as e:
        typer.secho(f"❌ Error generating SQL: {e}", fg=typer.colors.RED)
        raise typer.Exit(1)


# ── oxyde migrations <subcommand> ────────────────────────────────────────

migrations_app = typer.Typer(help="Migration maintenance commands")
app.add_typer(migrations_app, name="migrations")


@migrations_app.command("squash")
def squash(
    name: str = typer.Option(
        "squashed", help="Name suffix for the new initial migration"
    ),
    yes: bool = typer.Option(False, "--yes", "-y", help="Skip the confirmation prompt"),
) -> None:
    """
    Squash the whole migration history into a single initial migration.

    Replays all migration files in memory, computes the final schema and
    replaces the history with one 0001_<name>.py in the current format.
    Old files are deleted (version control is your backup). Manual SQL
    (ctx.execute) is NOT carried over — affected files are listed.

    On already-deployed databases run `oxyde migrate --fake` afterwards to
    record the new initial migration without executing it.
    """
    from oxyde.migrations.squash import squash_migrations

    config = load_config_or_exit()
    migrations_path = Path(config.migrations_dir)
    files = sorted(migrations_path.glob("[0-9]*.py"))

    if not files:
        typer.secho(
            "Nothing to squash: no migration files found.", fg=typer.colors.YELLOW
        )
        raise typer.Exit(0)

    typer.echo(f"Will squash {len(files)} migration file(s) into one:")
    for f in files:
        typer.echo(f"  • {f.name}")
    typer.echo()

    if not yes and not typer.confirm(
        "Delete these files and write a new 0001 migration?"
    ):
        raise typer.Exit(0)

    try:
        result = squash_migrations(config.migrations_dir, name=name)
    except Exception as e:
        typer.secho(f"❌ Squash failed: {e}", fg=typer.colors.RED)
        raise typer.Exit(1) from None

    assert result.new_file is not None
    typer.secho(
        f"✅ Created {result.new_file.name} ({result.table_count} table(s)), "
        f"deleted {len(result.deleted_files)} file(s).",
        fg=typer.colors.GREEN,
    )

    if result.raw_sql_files:
        typer.echo()
        typer.secho(
            "⚠️  These files contained manual SQL (ctx.execute) that was NOT "
            "carried over — move it manually if still needed:",
            fg=typer.colors.YELLOW,
        )
        for fname in result.raw_sql_files:
            typer.echo(f"  • {fname}")

    typer.echo()
    typer.echo("Next steps:")
    typer.echo("  • fresh databases:    oxyde migrate")
    typer.echo(
        "  • deployed databases: oxyde migrate --fake   (records 0001 without executing)"
    )

"""CLI commands for Oxyde migrations."""

from pathlib import Path

import typer

app = typer.Typer(
    name="oxyde",
    help="Oxyde ORM - Database migration and management tool",
    no_args_is_help=True,
)


@app.command()
def makemigrations(
    name: str | None = typer.Option(None, help="Migration name"),
    dry_run: bool = typer.Option(
        False, help="Show what would be created without actually creating"
    ),
    migrations_dir: str = typer.Option("migrations", help="Migrations directory"),
    dialect: str = typer.Option(
        "sqlite", help="Database dialect (sqlite, postgres, mysql)"
    ),
):
    """
    Create migration files by comparing current models with replayed migrations.

    Scans all OxydeModel subclasses, replays existing migrations,
    computes diff, and generates a new migration file if changes detected.
    """
    import json

    from oxyde.core import migration_compute_diff
    from oxyde.migrations import (
        extract_current_schema,
        generate_migration_file,
        replay_migrations,
    )

    typer.echo("📝 Creating migrations...")
    typer.echo()

    # Step 1: Extract current schema from models
    typer.echo("1️⃣  Extracting schema from models...")
    try:
        current_schema = extract_current_schema(dialect=dialect)
        table_count = len(current_schema["tables"])
        tables = ", ".join(current_schema["tables"].keys())
        typer.echo(f"   ✅ Found {table_count} table(s): {tables}")
    except Exception as e:
        typer.secho(f"   ❌ Error extracting schema: {e}", fg=typer.colors.RED)
        raise typer.Exit(1)

    # Step 2: Replay existing migrations
    typer.echo()
    typer.echo("2️⃣  Replaying existing migrations...")
    migrations_path = Path(migrations_dir)
    if not migrations_path.exists():
        typer.echo(f"   📁 Creating migrations directory: {migrations_path.absolute()}")
        if not dry_run:
            migrations_path.mkdir(parents=True, exist_ok=True)
        old_schema = {"version": 1, "tables": {}}
    else:
        try:
            old_schema = replay_migrations(migrations_dir)
            migration_count = len(list(migrations_path.glob("[0-9]*.py")))
            typer.echo(f"   ✅ Replayed {migration_count} migration(s)")
        except Exception as e:
            typer.secho(
                f"   ⚠️  Error replaying migrations: {e}", fg=typer.colors.YELLOW
            )
            typer.echo("   Using empty schema as baseline")
            old_schema = {"version": 1, "tables": {}}

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
            else:
                typer.echo(f"      - {op_type}")

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
                migrations_dir=migrations_dir,
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
                f"   ⚠️  Warning: Could not generate stubs: {e}", fg=typer.colors.YELLOW
            )
            # Don't fail migration on stub generation errors


@app.command()
def migrate(
    target: str | None = typer.Option(None, help="Target migration name"),
    fake: bool = typer.Option(
        False, help="Mark migrations as applied without running SQL"
    ),
    migrations_dir: str = typer.Option("migrations", help="Migrations directory"),
    db_alias: str = typer.Option("default", help="Database connection alias"),
):
    """
    Apply all pending migrations.

    Runs all migrations that haven't been applied yet.
    Use --target to migrate to a specific migration.
    """
    import asyncio

    from oxyde.migrations import (
        apply_migrations,
        get_applied_migrations,
        get_pending_migrations,
    )

    typer.echo("🚀 Applying migrations...")
    typer.echo()

    if fake:
        typer.secho(
            "⚠️  [FAKE MODE] Marking migrations as applied without executing SQL",
            fg=typer.colors.YELLOW,
        )
        typer.echo()

    try:
        # Get current state
        applied = asyncio.run(get_applied_migrations(db_alias))
        pending = get_pending_migrations(migrations_dir, applied)

        if not pending:
            typer.secho("✨ No pending migrations", fg=typer.colors.GREEN)
            return

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

        applied_migrations = asyncio.run(
            apply_migrations(
                migrations_dir=migrations_dir,
                db_alias=db_alias,
                target=target,
                fake=fake,
            )
        )

        typer.echo()
        typer.secho(
            f"✅ Applied {len(applied_migrations)} migration(s)",
            fg=typer.colors.GREEN,
            bold=True,
        )
        for name in applied_migrations:
            typer.echo(f"   - {name}")

    except Exception as e:
        typer.secho(f"❌ Error applying migrations: {e}", fg=typer.colors.RED)
        raise typer.Exit(1)


@app.command()
def showmigrations(
    migrations_dir: str = typer.Option("migrations", help="Migrations directory"),
    db_alias: str = typer.Option("default", help="Database connection alias"),
):
    """
    Show list of all migrations with their status (applied/pending).
    """
    import asyncio

    from oxyde.migrations import get_applied_migrations, get_migration_files

    typer.echo("📋 Migrations status:")
    typer.echo()

    try:
        # Get applied migrations
        applied = asyncio.run(get_applied_migrations(db_alias))
        applied_set = set(applied)

        # Get all migration files
        all_migrations = get_migration_files(migrations_dir)

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
    migrations_dir: str = typer.Option("migrations", help="Migrations directory"),
    dialect: str = typer.Option(
        "sqlite", help="Database dialect (sqlite, postgres, mysql)"
    ),
):
    """
    Print the SQL for a specific migration.

    Args:
        name: Name of the migration file (e.g., 0001_initial)
    """
    import json
    from pathlib import Path

    from oxyde.core import migration_to_sql
    from oxyde.migrations.context import MigrationContext
    from oxyde.migrations.executor import import_migration_module

    typer.echo(f"📝 SQL for migration: {name}")
    typer.echo()

    try:
        # Find migration file
        migration_path = Path(migrations_dir) / f"{name}.py"
        if not migration_path.exists():
            typer.secho(f"❌ Migration not found: {name}", fg=typer.colors.RED)
            raise typer.Exit(1)

        # Import migration module
        module = import_migration_module(migration_path)

        if not hasattr(module, "upgrade"):
            typer.secho("❌ Migration missing upgrade() function", fg=typer.colors.RED)
            raise typer.Exit(1)

        # Create context in collect mode to get operations
        ctx = MigrationContext(mode="collect", dialect=dialect)
        module.upgrade(ctx)

        operations = ctx.get_collected_operations()

        if not operations:
            typer.echo("-- No operations (empty migration)")
            return

        # Convert operations to SQL
        operations_json = json.dumps(operations)
        sql_statements = migration_to_sql(operations_json, dialect)

        # Print SQL
        typer.secho(f"-- Generated SQL ({dialect}):", fg=typer.colors.CYAN)
        typer.echo()
        for sql in sql_statements:
            typer.echo(sql)
            typer.echo()

    except Exception as e:
        typer.secho(f"❌ Error generating SQL: {e}", fg=typer.colors.RED)
        raise typer.Exit(1)


@app.command()
def rollback(
    steps: int = typer.Option(1, help="Number of migrations to roll back"),
    migrations_dir: str = typer.Option("migrations", help="Migrations directory"),
    db_alias: str = typer.Option("default", help="Database connection alias"),
    fake: bool = typer.Option(False, help="Remove from history without running SQL"),
):
    """
    Roll back the last N migrations.

    Args:
        steps: Number of migrations to roll back (default: 1)
    """
    import asyncio

    from oxyde.migrations import get_applied_migrations, rollback_migrations

    typer.echo(f"🔙 Rolling back {steps} migration(s)...")
    typer.echo()

    if fake:
        typer.secho(
            "⚠️  [FAKE MODE] Removing from history without executing SQL",
            fg=typer.colors.YELLOW,
        )
        typer.echo()

    try:
        # Get applied migrations
        applied = asyncio.run(get_applied_migrations(db_alias))

        if not applied:
            typer.secho("✨ No migrations to roll back", fg=typer.colors.YELLOW)
            return

        # Show what will be rolled back
        to_rollback = applied[-steps:] if steps < len(applied) else applied
        typer.echo(f"Will roll back {len(to_rollback)} migration(s):")
        for name in reversed(to_rollback):
            typer.echo(f"  - {name}")

        typer.echo()

        # Roll back migrations
        rolled_back = asyncio.run(
            rollback_migrations(
                steps=steps,
                migrations_dir=migrations_dir,
                db_alias=db_alias,
                fake=fake,
            )
        )

        typer.echo()
        typer.secho(
            f"✅ Rolled back {len(rolled_back)} migration(s)",
            fg=typer.colors.GREEN,
            bold=True,
        )
        for name in rolled_back:
            typer.echo(f"   - {name}")

    except Exception as e:
        typer.secho(f"❌ Error rolling back migrations: {e}", fg=typer.colors.RED)
        raise typer.Exit(1)


@app.command(name="generate-stubs")
def generate_stubs():
    """
    Generate .pyi stub files for all registered table models.

    Creates type stub files with autocomplete support for QuerySet filter/exclude
    methods with all field lookups (contains, gt, gte, etc.).
    """
    from oxyde.codegen import generate_stubs_for_models, write_stubs
    from oxyde.models.registry import registered_tables

    typer.echo("🔧 Generating type stubs...")
    typer.echo()

    try:
        # Get all registered table models
        models = list(registered_tables().values())

        if not models:
            typer.secho("⚠️  No table models found", fg=typer.colors.YELLOW)
            typer.echo("Make sure your models are imported and registered")
            return

        typer.echo(f"Found {len(models)} table model(s):")
        for model in models:
            typer.echo(f"  - {model.__module__}.{model.__name__}")

        typer.echo()
        typer.echo("Generating stubs...")

        # Generate stubs
        stub_mapping = generate_stubs_for_models(models)

        # Write to disk
        write_stubs(stub_mapping)

        typer.echo()
        typer.secho(
            f"✅ Generated {len(stub_mapping)} stub file(s)",
            fg=typer.colors.GREEN,
            bold=True,
        )

    except Exception as e:
        typer.secho(f"❌ Error generating stubs: {e}", fg=typer.colors.RED)
        import traceback

        traceback.print_exc()
        raise typer.Exit(1)


if __name__ == "__main__":
    app()

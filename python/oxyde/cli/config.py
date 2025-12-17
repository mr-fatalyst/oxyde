"""Configuration commands (init)."""

import typer

from oxyde.cli.app import app
from oxyde.migrations.config import load_config, save_config


@app.command()
def init():
    """
    Initialize Oxyde configuration.

    Interactively asks for configuration values and creates oxyde_config.py.
    """
    typer.echo("🔧 Initializing Oxyde configuration...")
    typer.echo()

    # Check if config already exists
    existing = load_config()
    if existing:
        typer.secho("Found existing oxyde_config.py:", fg=typer.colors.YELLOW)
        typer.echo(f"  MODELS = {existing.models}")
        typer.echo(f"  DIALECT = {existing.dialect}")
        typer.echo(f"  MIGRATIONS_DIR = {existing.migrations_dir}")
        typer.echo(f"  DATABASES = {existing.databases}")
        typer.echo()
        if not typer.confirm("Overwrite existing configuration?"):
            typer.echo("Aborted.")
            raise typer.Exit(0)
        typer.echo()

    # Ask for models
    typer.echo("📦 Models configuration")
    typer.echo("   Enter Python modules containing your OxydeModel classes.")
    typer.echo("   Examples: 'models', 'app.models', 'myapp.db.models'")
    typer.echo()

    models_input = typer.prompt(
        "   Models module(s)",
        default="models",
    )
    # Parse comma or space separated list
    models = [m.strip() for m in models_input.replace(",", " ").split() if m.strip()]

    typer.echo()

    # Ask for dialect
    typer.echo("🗄️  Database dialect")
    dialect = typer.prompt(
        "   Dialect (postgres/sqlite/mysql)",
        default="postgres",
    )
    if dialect not in ("postgres", "sqlite", "mysql"):
        typer.secho(
            f"   ⚠️  Unknown dialect '{dialect}', using anyway", fg=typer.colors.YELLOW
        )

    typer.echo()

    # Ask for database URL
    typer.echo("🔗 Database connection")
    default_url = typer.prompt(
        "   Database URL",
        default="postgresql://localhost/mydb",
    )

    typer.echo()

    # Ask for migrations directory
    migrations_dir = typer.prompt(
        "📁 Migrations directory",
        default="migrations",
    )

    typer.echo()

    # Save config
    databases = {"default": default_url}
    save_config(
        models=models,
        dialect=dialect,
        migrations_dir=migrations_dir,
        databases=databases,
    )

    typer.secho(
        "✅ Configuration saved to oxyde_config.py", fg=typer.colors.GREEN, bold=True
    )
    typer.echo()
    typer.echo("Next steps:")
    typer.echo("  1. Edit oxyde_config.py to adjust settings if needed")
    typer.echo("  2. Create your models in the specified module")
    typer.echo("  3. Run 'oxyde makemigrations' to generate migrations")
    typer.echo("  4. Run 'oxyde migrate' to apply migrations")

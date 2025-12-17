"""Main CLI application and shared utilities."""

from pathlib import Path

import typer

from oxyde.migrations.config import OxydeConfig, load_config

# Main application
app = typer.Typer(
    name="oxyde",
    help="Oxyde ORM - Database migration and management tool",
    no_args_is_help=True,
)


def load_config_or_exit() -> OxydeConfig:
    """Load config from oxyde_config.py or exit with error.

    Returns:
        OxydeConfig instance
    """
    config = load_config()
    if config is None:
        typer.secho(
            "❌ No oxyde_config.py found",
            fg=typer.colors.RED,
        )
        typer.echo()
        typer.echo("Run 'oxyde init' to create configuration.")
        raise typer.Exit(1)

    if not config.models:
        typer.secho(
            "❌ No models configured in oxyde_config.py",
            fg=typer.colors.RED,
        )
        typer.echo()
        typer.echo("Add models to oxyde_config.py:")
        typer.secho('  MODELS = ["models", "app.models"]', fg=typer.colors.CYAN)
        raise typer.Exit(1)

    return config


def require_databases(config: OxydeConfig, db_alias: str = "default") -> None:
    """Validate that databases are configured and alias exists.

    Args:
        config: OxydeConfig instance
        db_alias: Database alias to check

    Raises:
        typer.Exit: If validation fails
    """
    if not config.databases:
        typer.secho(
            "❌ No databases configured in oxyde_config.py",
            fg=typer.colors.RED,
        )
        typer.echo()
        typer.echo("Add DATABASES to oxyde_config.py:")
        typer.secho(
            '  DATABASES = {"default": "postgresql://localhost/mydb"}',
            fg=typer.colors.CYAN,
        )
        raise typer.Exit(1)

    if db_alias not in config.databases:
        typer.secho(
            f"❌ Database '{db_alias}' not found in oxyde_config.py",
            fg=typer.colors.RED,
        )
        typer.echo()
        typer.echo(f"Available databases: {list(config.databases.keys())}")
        raise typer.Exit(1)


async def init_databases(databases: dict[str, str]) -> None:
    """Initialize database connections from config.

    Args:
        databases: Dict mapping alias to connection URL
    """
    from oxyde.db import AsyncDatabase

    for name, url in databases.items():
        db = AsyncDatabase(url, name=name)
        await db.connect()


def ensure_migrations_dir(migrations_dir: str | Path, dry_run: bool = False) -> Path:
    """Ensure migrations directory exists.

    Args:
        migrations_dir: Path to migrations directory
        dry_run: If True, don't actually create directory

    Returns:
        Path to migrations directory
    """
    path = Path(migrations_dir)
    if not path.exists():
        typer.echo(f"   📁 Creating migrations directory: {path.absolute()}")
        if not dry_run:
            path.mkdir(parents=True, exist_ok=True)
    return path

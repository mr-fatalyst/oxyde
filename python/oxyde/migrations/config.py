"""Configuration loading from oxyde_config.py."""

from __future__ import annotations

import importlib
import sys
from dataclasses import dataclass, field
from pathlib import Path

import typer


@dataclass
class OxydeConfig:
    """Oxyde configuration."""

    models: list[str] = field(default_factory=list)
    dialect: str = "postgres"
    migrations_dir: str = "migrations"
    databases: dict[str, str] = field(default_factory=dict)


def load_config() -> OxydeConfig | None:
    """Load configuration from oxyde_config.py.

    Returns:
        OxydeConfig if oxyde_config.py exists and is valid, None otherwise.
    """
    config_path = Path("oxyde_config.py")

    if not config_path.exists():
        return None

    # Add current directory to path for import
    if "." not in sys.path:
        sys.path.insert(0, ".")

    try:
        # Import the config module
        if "oxyde_config" in sys.modules:
            # Reload if already imported
            importlib.reload(sys.modules["oxyde_config"])
            config_module = sys.modules["oxyde_config"]
        else:
            config_module = importlib.import_module("oxyde_config")

        # Extract config values
        models = getattr(config_module, "MODELS", [])
        dialect = getattr(config_module, "DIALECT", "postgres")
        migrations_dir = getattr(config_module, "MIGRATIONS_DIR", "migrations")
        databases = getattr(config_module, "DATABASES", {})

        return OxydeConfig(
            models=models,
            dialect=dialect,
            migrations_dir=migrations_dir,
            databases=databases,
        )

    except Exception:
        return None


def import_models(modules: list[str]) -> int:
    """Import modules to register models.

    Args:
        modules: List of module names to import.

    Returns:
        Number of successfully imported modules.
    """
    # Add current directory to path for local imports
    if "." not in sys.path:
        sys.path.insert(0, ".")

    imported = 0
    for module_name in modules:
        try:
            importlib.import_module(module_name)
            imported += 1
        except ImportError as e:
            typer.secho(
                f"   ⚠️  Failed to import '{module_name}': {e}",
                fg=typer.colors.YELLOW,
            )

    return imported


def generate_config_file(
    models: list[str],
    dialect: str = "postgres",
    migrations_dir: str = "migrations",
    databases: dict[str, str] | None = None,
) -> str:
    """Generate oxyde_config.py content.

    Args:
        models: List of model modules.
        dialect: Database dialect.
        migrations_dir: Migrations directory.
        databases: Database connection URLs.

    Returns:
        Generated Python file content.
    """
    if databases is None:
        databases = {"default": "postgresql://localhost/mydb"}

    models_str = ", ".join(f'"{m}"' for m in models)

    # Format databases dict
    db_lines = []
    for name, url in databases.items():
        db_lines.append(f'    "{name}": "{url}",')
    databases_str = "\n".join(db_lines)

    return f'''"""Oxyde ORM configuration."""

# List of Python modules containing OxydeModel classes
MODELS = [{models_str}]

# Database dialect: "postgres", "sqlite", or "mysql"
DIALECT = "{dialect}"

# Directory for migration files
MIGRATIONS_DIR = "{migrations_dir}"

# Database connections
# Keys are connection aliases, values are connection URLs
DATABASES = {{
{databases_str}
}}
'''


def save_config(
    models: list[str],
    dialect: str = "postgres",
    migrations_dir: str = "migrations",
    databases: dict[str, str] | None = None,
    path: Path | None = None,
) -> Path:
    """Save configuration to oxyde_config.py.

    Args:
        models: List of model modules.
        dialect: Database dialect.
        migrations_dir: Migrations directory.
        databases: Database connection URLs.
        path: Path to config file. If None, uses current directory.

    Returns:
        Path to the created config file.
    """
    if path is None:
        path = Path("oxyde_config.py")

    content = generate_config_file(
        models=models,
        dialect=dialect,
        migrations_dir=migrations_dir,
        databases=databases,
    )

    path.write_text(content, encoding="utf-8")
    return path


__all__ = [
    "OxydeConfig",
    "load_config",
    "import_models",
    "generate_config_file",
    "save_config",
]

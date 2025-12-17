"""Oxyde CLI - Database migration and management tool."""

# Import command modules to register them with the app (side-effect: registers commands)
from oxyde.cli import codegen, config, migrations  # noqa: F401
from oxyde.cli.app import app

__all__ = ["app"]

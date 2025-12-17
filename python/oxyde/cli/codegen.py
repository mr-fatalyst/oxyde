"""Code generation commands (generate-stubs)."""

import typer

from oxyde.cli.app import app, load_config_or_exit
from oxyde.migrations.config import import_models


@app.command(name="generate-stubs")
def generate_stubs():
    """
    Generate .pyi stub files for all registered table models.

    Creates type stub files with autocomplete support for QuerySet filter/exclude
    methods with all field lookups (contains, gt, gte, etc.).
    """
    from oxyde.codegen import generate_stubs_for_models, write_stubs
    from oxyde.models.registry import registered_tables

    # Load config and import models
    config = load_config_or_exit()

    typer.echo("🔧 Generating type stubs...")
    typer.echo()

    # Import models first
    imported = import_models(config.models)
    if imported == 0:
        typer.secho("❌ No modules imported", fg=typer.colors.RED)
        raise typer.Exit(1)

    try:
        # Get all registered table models
        models = list(registered_tables().values())

        if not models:
            typer.secho("⚠️  No table models found", fg=typer.colors.YELLOW)
            typer.echo("Make sure your models have 'class Meta: is_table = True'")
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

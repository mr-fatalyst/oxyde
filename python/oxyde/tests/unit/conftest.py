"""Unit test fixtures — cleanup registry automatically."""
import pytest

from oxyde.models.registry import clear_registry


@pytest.fixture(autouse=True)
def _cleanup_registry():
    """Auto-cleanup model registry for every unit test."""
    clear_registry()
    yield
    clear_registry()

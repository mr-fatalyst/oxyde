"""Dynamic import of _oxyde_core Rust extension with ABI version checking.

This module handles the import of the Rust PyO3 extension module (_oxyde_core)
and provides stub functions if the module is not available or incompatible.

ABI Version:
    EXPECTED_ABI_VERSION must match __abi_version__ in the Rust module.
    If mismatch, ImportError is raised prompting rebuild.

Import Strategy:
    1. Try to import _oxyde_core
    2. Check ABI version
    3. Export available functions with hasattr() checks for backwards compat
    4. If import fails, create stub functions that raise RuntimeError

Why Stub Functions?
    Allows Python code to import oxyde even without the Rust module built.
    Useful for:
    - IDE autocomplete without full build
    - Documentation generation
    - Partial testing (model definitions, IR building)

Functions Exported:
    Pool management:
        init_pool, init_pool_overwrite, close_pool, close_all_pools

    Query execution:
        execute, execute_in_transaction

    Transactions:
        begin_transaction, commit_transaction, rollback_transaction
        create_savepoint, rollback_to_savepoint, release_savepoint

    Debugging:
        render_sql, render_sql_debug, explain_query

    Migrations:
        migration_compute_diff, migration_to_sql

    Validation:
        register_validator (no-op, not implemented in Rust)
"""

from __future__ import annotations

import sys
from collections.abc import Callable
from typing import Any

# Expected ABI version - must match __abi_version__ in Rust module
EXPECTED_ABI_VERSION = 1

# Function registry: (rust_name, export_name, is_async, is_noop_fallback)
# - rust_name: name in _oxyde_core module
# - export_name: name to export (usually same, but explain -> explain_query)
# - is_async: True for async functions
# - is_noop_fallback: True if stub should be no-op instead of raising
_FUNCTION_REGISTRY: list[tuple[str, str, bool, bool]] = [
    # Pool management
    ("init_pool", "init_pool", True, False),
    ("init_pool_overwrite", "init_pool_overwrite", True, False),
    ("close_pool", "close_pool", True, False),
    ("close_all_pools", "close_all_pools", True, False),
    # Query execution
    ("execute", "execute", True, False),
    ("execute_to_pylist", "execute_to_pylist", True, False),
    ("execute_select_direct", "execute_select_direct", True, False),
    ("execute_select_batched", "execute_select_batched", True, False),
    ("execute_select_batched_dedup", "execute_select_batched_dedup", True, False),
    ("execute_in_transaction", "execute_in_transaction", True, False),
    # Transactions
    ("begin_transaction", "begin_transaction", True, False),
    ("commit_transaction", "commit_transaction", True, False),
    ("rollback_transaction", "rollback_transaction", True, False),
    ("create_savepoint", "create_savepoint", True, False),
    ("rollback_to_savepoint", "rollback_to_savepoint", True, False),
    ("release_savepoint", "release_savepoint", True, False),
    # Debug/introspection
    ("render_sql", "render_sql", True, False),
    ("render_sql_debug", "render_sql_debug", False, False),
    ("explain", "explain_query", True, False),  # Renamed for API consistency
    # Migrations
    ("migration_compute_diff", "migration_compute_diff", False, False),
    ("migration_to_sql", "migration_to_sql", False, False),
    # Validation (no-op - not implemented in Rust, kept for compatibility)
    ("register_validator", "register_validator", False, True),
]


def _make_stub(name: str, is_async: bool, is_noop: bool) -> Callable[..., Any]:
    """Create a stub function that either raises RuntimeError or is a no-op."""
    if is_noop:

        def noop_stub(*args: Any, **kwargs: Any) -> None:
            pass

        noop_stub.__name__ = name
        noop_stub.__doc__ = f"No-op stub for {name} (Rust module not available)"
        return noop_stub

    error_msg = (
        f"Rust core module not available or missing '{name}'. "
        "Please rebuild with: maturin develop --release"
    )

    if is_async:

        async def async_stub(*args: Any, **kwargs: Any) -> Any:
            raise RuntimeError(error_msg)

        async_stub.__name__ = name
        async_stub.__doc__ = f"Stub for {name} - raises RuntimeError"
        return async_stub
    else:

        def sync_stub(*args: Any, **kwargs: Any) -> Any:
            raise RuntimeError(error_msg)

        sync_stub.__name__ = name
        sync_stub.__doc__ = f"Stub for {name} - raises RuntimeError"
        return sync_stub


def _load_functions() -> dict[str, Callable[..., Any]]:
    """Load functions from Rust module or create stubs."""
    exports: dict[str, Callable[..., Any]] = {}

    try:
        import _oxyde_core

        # ABI version check
        abi_version = getattr(_oxyde_core, "__abi_version__", None)
        if abi_version is None:
            raise ImportError("Rust core module does not expose __abi_version__")
        if abi_version != EXPECTED_ABI_VERSION:
            raise ImportError(
                f"ABI version mismatch: expected {EXPECTED_ABI_VERSION}, "
                f"got {abi_version}. Please reinstall oxyde-core."
            )

        # Load each function from registry
        for rust_name, export_name, is_async, is_noop in _FUNCTION_REGISTRY:
            if hasattr(_oxyde_core, rust_name):
                exports[export_name] = getattr(_oxyde_core, rust_name)
            else:
                # Function missing in Rust module (older version?)
                exports[export_name] = _make_stub(export_name, is_async, is_noop)

    except ImportError as e:
        print(f"Warning: Failed to import Rust core module: {e}", file=sys.stderr)
        print("Oxyde ORM will not be fully functional.", file=sys.stderr)

        # Create all stubs
        for _, export_name, is_async, is_noop in _FUNCTION_REGISTRY:
            exports[export_name] = _make_stub(export_name, is_async, is_noop)

    return exports


# Load functions once at module import
_exports = _load_functions()

# Export individual functions for IDE support and type checking
execute = _exports["execute"]
execute_to_pylist = _exports["execute_to_pylist"]
execute_select_direct = _exports["execute_select_direct"]
execute_select_batched = _exports["execute_select_batched"]
execute_select_batched_dedup = _exports["execute_select_batched_dedup"]
execute_in_transaction = _exports["execute_in_transaction"]
init_pool = _exports["init_pool"]
init_pool_overwrite = _exports["init_pool_overwrite"]
close_pool = _exports["close_pool"]
close_all_pools = _exports["close_all_pools"]
begin_transaction = _exports["begin_transaction"]
commit_transaction = _exports["commit_transaction"]
rollback_transaction = _exports["rollback_transaction"]
create_savepoint = _exports["create_savepoint"]
rollback_to_savepoint = _exports["rollback_to_savepoint"]
release_savepoint = _exports["release_savepoint"]
register_validator = _exports["register_validator"]
render_sql = _exports["render_sql"]
render_sql_debug = _exports["render_sql_debug"]
explain_query = _exports["explain_query"]
migration_compute_diff = _exports["migration_compute_diff"]
migration_to_sql = _exports["migration_to_sql"]

__all__ = [
    "execute",
    "execute_to_pylist",
    "execute_select_direct",
    "execute_select_batched",
    "execute_select_batched_dedup",
    "execute_in_transaction",
    "init_pool",
    "init_pool_overwrite",
    "close_pool",
    "close_all_pools",
    "begin_transaction",
    "commit_transaction",
    "rollback_transaction",
    "create_savepoint",
    "rollback_to_savepoint",
    "release_savepoint",
    "register_validator",
    "render_sql",
    "render_sql_debug",
    "explain_query",
    "migration_compute_diff",
    "migration_to_sql",
]

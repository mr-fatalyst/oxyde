"""End-to-end typecheck tests: generate stubs, run mypy on usage samples."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import oxyde
import pytest

FIXTURES_DIR = Path(__file__).parent / "fixtures"
OXYDE_PARENT = str(Path(oxyde.__file__).parent.parent)


FIXTURES: list[tuple[str, str, str, str]] = [
    # (test_id, fixture_rel_dir, model_module_basename, usage_file_basename)
    ("smoke", "smoke", "tiny_model", "tiny_usage.py"),
    ("kitchen_sink", "kitchen_sink", "models", "usage.py"),
    ("edges/helpers_only", "edges/helpers_only", "module", "usage.py"),
    ("edges/mixed_module", "edges/mixed_module", "module", "usage.py"),
    ("edges/toplevel_dataclass", "edges/toplevel_dataclass", "module", "usage.py"),
    ("edges/toplevel_overload", "edges/toplevel_overload", "module", "usage.py"),
    (
        "edges/type_checking_imports",
        "edges/type_checking_imports",
        "module",
        "usage.py",
    ),
    ("edges/forward_ref", "edges/forward_ref", "module", "usage.py"),
    ("edges/forward_ref_cross", "edges/forward_ref_cross", "module", "usage.py"),
    ("edges/inheritance_chain", "edges/inheritance_chain", "module", "usage.py"),
    ("edges/reserved_names", "edges/reserved_names", "module", "usage.py"),
    ("edges/generics_in_field", "edges/generics_in_field", "module", "usage.py"),
]


def _run_mypy(target: Path, cwd: Path) -> subprocess.CompletedProcess[str]:
    env = {**os.environ, "MYPYPATH": OXYDE_PARENT}
    return subprocess.run(
        [
            sys.executable,
            "-m",
            "mypy",
            "--no-incremental",
            "--explicit-package-bases",
            "--show-error-codes",
            str(target),
        ],
        capture_output=True,
        text=True,
        cwd=cwd,
        env=env,
    )


@pytest.mark.parametrize(
    ("fixture_dir", "model_module", "usage_file"),
    [(f[1], f[2], f[3]) for f in FIXTURES],
    ids=[f[0] for f in FIXTURES],
)
def test_mypy_accepts_generated_stubs(
    fixture_dir: str,
    model_module: str,
    usage_file: str,
    generate_stubs,
) -> None:
    source_dir = FIXTURES_DIR / fixture_dir
    work_dir = generate_stubs(source_dir, model_module)
    usage_path = work_dir / usage_file
    assert usage_path.exists(), f"usage fixture missing: {usage_path}"

    result = _run_mypy(usage_path, work_dir)
    assert result.returncode == 0, (
        f"mypy failed for fixture '{fixture_dir}':\n"
        f"--- STDOUT ---\n{result.stdout}\n"
        f"--- STDERR ---\n{result.stderr}"
    )

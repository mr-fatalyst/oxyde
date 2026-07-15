"""End-to-end typecheck tests: generate stubs, run mypy/pyright/ty on them."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import oxyde
import pytest

FIXTURES_DIR = Path(__file__).parent / "fixtures"
OXYDE_PARENT = str(Path(oxyde.__file__).parent.parent)

PYRIGHT_EXE = shutil.which("pyright")
TY_EXE = shutil.which("ty")


FIXTURES: list[tuple[str, str, str, str]] = [
    # (test_id, fixture_rel_dir, model_module_basename, usage_file_basename)
    ("smoke", "smoke", "tiny_model", "tiny_usage.py"),
    ("kitchen_sink", "kitchen_sink", "models", "usage.py"),
    ("edges/helpers_only", "edges/helpers_only", "module", "usage.py"),
    ("edges/mixed_module", "edges/mixed_module", "module", "usage.py"),
    ("edges/toplevel_overload", "edges/toplevel_overload", "module", "usage.py"),
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


def _stub_targets(work_dir: Path, usage_file: str) -> list[Path]:
    """Usage sample plus the generated .pyi files themselves."""
    usage_path = work_dir / usage_file
    assert usage_path.exists(), f"usage fixture missing: {usage_path}"
    return [usage_path, *sorted(work_dir.glob("*.pyi"))]


def _run_pyright(targets: list[Path], cwd: Path) -> subprocess.CompletedProcess[str]:
    (cwd / "pyrightconfig.json").write_text(
        json.dumps({"typeCheckingMode": "standard", "extraPaths": [OXYDE_PARENT]})
    )
    return subprocess.run(
        [PYRIGHT_EXE, "--pythonpath", sys.executable, *map(str, targets)],
        capture_output=True,
        text=True,
        cwd=cwd,
    )


def _run_ty(targets: list[Path], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            TY_EXE,
            "check",
            "--python",
            sys.prefix,
            "--extra-search-path",
            OXYDE_PARENT,
            "--extra-search-path",
            ".",
            *map(str, targets),
        ],
        capture_output=True,
        text=True,
        cwd=cwd,
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


@pytest.mark.skipif(PYRIGHT_EXE is None, reason="pyright is not installed")
@pytest.mark.parametrize(
    ("fixture_dir", "model_module", "usage_file"),
    [(f[1], f[2], f[3]) for f in FIXTURES],
    ids=[f[0] for f in FIXTURES],
)
def test_pyright_accepts_generated_stubs(
    fixture_dir: str,
    model_module: str,
    usage_file: str,
    generate_stubs,
) -> None:
    """pyright (standard mode) must accept the usage file AND the stubs."""
    source_dir = FIXTURES_DIR / fixture_dir
    work_dir = generate_stubs(source_dir, model_module)

    result = _run_pyright(_stub_targets(work_dir, usage_file), work_dir)
    assert result.returncode == 0, (
        f"pyright failed for fixture '{fixture_dir}':\n"
        f"--- STDOUT ---\n{result.stdout}\n"
        f"--- STDERR ---\n{result.stderr}"
    )


@pytest.mark.skipif(TY_EXE is None, reason="ty is not installed")
@pytest.mark.parametrize(
    ("fixture_dir", "model_module", "usage_file"),
    [(f[1], f[2], f[3]) for f in FIXTURES],
    ids=[f[0] for f in FIXTURES],
)
def test_ty_accepts_generated_stubs(
    fixture_dir: str,
    model_module: str,
    usage_file: str,
    generate_stubs,
) -> None:
    """ty must accept the usage file AND the stubs."""
    source_dir = FIXTURES_DIR / fixture_dir
    work_dir = generate_stubs(source_dir, model_module)

    result = _run_ty(_stub_targets(work_dir, usage_file), work_dir)
    assert result.returncode == 0, (
        f"ty failed for fixture '{fixture_dir}':\n"
        f"--- STDOUT ---\n{result.stdout}\n"
        f"--- STDERR ---\n{result.stderr}"
    )


# Per-fixture marks for the model-source check. ``mixed_module`` calls
# ``Note.objects.all()`` from inside the model file itself, which exercises
# QueryManager/Query typing rather than the Field-assignment fix this suite
# is about. Stubs cover the cross-module case; in-file manager calls remain
# untyped until QueryManager/Query gain proper Generic[TModel] propagation.
_MODEL_SOURCE_XFAIL = {
    "edges/mixed_module": pytest.mark.xfail(
        strict=True,
        reason=(
            "QueryManager.all() declared as Coroutine[..., bytes | list[Any]] "
            "and Model.objects is ClassVar[QueryManager] without TModel. "
            "Stubs cover cross-module imports; in-file Model.objects.xxx() "
            "calls require Generic[TModel] propagation through the manager, "
            "Query and ExecutionMixin. Tracked as a follow-up to issue #13."
        ),
    ),
}


def _model_source_params() -> list:
    params = []
    for test_id, fixture_dir, model_module, usage_file in FIXTURES:
        marks = (_MODEL_SOURCE_XFAIL[test_id],) if test_id in _MODEL_SOURCE_XFAIL else ()
        params.append(
            pytest.param(fixture_dir, model_module, usage_file, id=test_id, marks=marks)
        )
    return params


@pytest.mark.parametrize(
    ("fixture_dir", "model_module", "usage_file"),
    _model_source_params(),
)
def test_mypy_accepts_model_source(
    fixture_dir: str,
    model_module: str,
    usage_file: str,
    generate_stubs,
) -> None:
    """Mypy must accept the model source file itself, not just the usage stub.

    Reproduces issue #13: pyright/ty (and mypy without the pydantic plugin)
    flag every ``field: T = Field(...)`` assignment because ``Field()`` returns
    ``OxydeFieldInfo`` rather than ``Any``. The existing typecheck e2e only
    runs mypy on the usage file, which resolves through the generated ``.pyi``
    and hides the problem.
    """
    source_dir = FIXTURES_DIR / fixture_dir
    work_dir = generate_stubs(source_dir, model_module)
    model_path = work_dir / f"{model_module}.py"
    assert model_path.exists(), f"model fixture missing: {model_path}"

    result = _run_mypy(model_path, work_dir)
    assert result.returncode == 0, (
        f"mypy failed for fixture '{fixture_dir}':\n"
        f"--- STDOUT ---\n{result.stdout}\n"
        f"--- STDERR ---\n{result.stderr}"
    )

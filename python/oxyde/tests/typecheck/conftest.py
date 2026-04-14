"""Fixtures for typecheck end-to-end tests.

Copies fixture source files to tmp_path, imports the model module dynamically,
generates .pyi stubs next to it, then yields the directory for mypy to chew on.
Cleans registry + sys.modules after each test so fixtures don't leak.
"""

from __future__ import annotations

import importlib.util
import inspect
import shutil
import sys
from collections.abc import Callable, Iterator
from pathlib import Path

import pytest

from oxyde.codegen.stub_generator import generate_stubs_for_models, write_stubs
from oxyde.models.registry import clear_registry, finalize_pending, registered_tables


def _import_from_path(module_name: str, file_path: Path) -> None:
    spec = importlib.util.spec_from_file_location(module_name, file_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load spec for {file_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)


@pytest.fixture
def generate_stubs(tmp_path: Path) -> Iterator[Callable[[Path, str], Path]]:
    """Copy fixture dir to tmp_path, import model modules, write .pyi stubs.

    Returns a callable: (source_dir, model_module_basename) -> tmp_path.
    model_module_basename is the .py filename (without extension) whose models
    should be registered first (e.g. "tiny_model").
    """
    injected_modules: list[str] = []
    tmp_on_path = str(tmp_path)
    path_inserted = False

    def _run(source_dir: Path, model_module: str) -> Path:
        nonlocal path_inserted
        shutil.copytree(source_dir, tmp_path, dirs_exist_ok=True)
        clear_registry()
        if tmp_on_path not in sys.path:
            sys.path.insert(0, tmp_on_path)
            path_inserted = True
        model_file = tmp_path / f"{model_module}.py"
        _import_from_path(model_module, model_file)
        injected_modules.append(model_module)
        finalize_pending()

        tmp_root = tmp_path.resolve()
        models = [
            m
            for m in registered_tables().values()
            if Path(inspect.getfile(m)).resolve().is_relative_to(tmp_root)
        ]
        if models:
            stub_mapping = generate_stubs_for_models(models)
            write_stubs(stub_mapping)
        return tmp_path

    yield _run

    for name in injected_modules:
        sys.modules.pop(name, None)
    if path_inserted and tmp_on_path in sys.path:
        sys.path.remove(tmp_on_path)
    clear_registry()

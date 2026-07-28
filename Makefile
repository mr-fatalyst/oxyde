.PHONY: test test-unit test-integration test-smoke test-rust coverage lint format build-core

PYTHON_DIR = python
TEST_DIR = $(PYTHON_DIR)/oxyde/tests
COV_PKG = $(PYTHON_DIR)/oxyde

test:
	pytest $(TEST_DIR)

test-unit:
	pytest $(TEST_DIR)/unit

test-integration:
	pytest $(TEST_DIR)/integration

test-smoke:
	pytest $(TEST_DIR)/smoke

test-rust:
	cargo test --workspace

coverage:
	pytest $(TEST_DIR) --cov=$(COV_PKG) --cov-report=term-missing

lint:
	pre-commit run --all-files

format:
	cd $(PYTHON_DIR) && ruff format .
	cargo fmt --all

build-core:
	cd crates/oxyde-core-py && maturin develop --release

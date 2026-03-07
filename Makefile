.PHONY: test test-unit test-integration test-smoke coverage lint format

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

coverage:
	pytest $(TEST_DIR) --cov=$(COV_PKG) --cov-report=term-missing

lint:
	cd $(PYTHON_DIR) && ruff check .

format:
	cd $(PYTHON_DIR) && ruff format .

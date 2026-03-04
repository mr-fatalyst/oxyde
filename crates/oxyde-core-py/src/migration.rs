//! Migration PyO3 wrappers: schema diff computation and SQL generation.

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

/// Compute diff between two schema snapshots (JSON)
///
/// Args:
///     old_json: Old schema snapshot as JSON string
///     new_json: New schema snapshot as JSON string
///
/// Returns:
///     JSON string with list of migration operations
#[pyfunction]
pub(crate) fn migration_compute_diff(old_json: &str, new_json: &str) -> PyResult<String> {
    use oxyde_migrate::{compute_diff, Snapshot};

    let old = Snapshot::from_json(old_json).map_err(|e| {
        PyErr::new::<PyValueError, _>(format!("Failed to parse old snapshot: {}", e))
    })?;

    let new = Snapshot::from_json(new_json).map_err(|e| {
        PyErr::new::<PyValueError, _>(format!("Failed to parse new snapshot: {}", e))
    })?;

    let ops = compute_diff(&old, &new);

    serde_json::to_string(&ops).map_err(|e| {
        PyErr::new::<PyValueError, _>(format!("Failed to serialize operations: {}", e))
    })
}

/// Convert migration operations to SQL statements
///
/// Args:
///     operations_json: JSON string with list of migration operations
///     dialect: Database dialect ("sqlite", "postgres", or "mysql")
///
/// Returns:
///     List of SQL statements
#[pyfunction]
pub(crate) fn migration_to_sql(operations_json: &str, dialect: &str) -> PyResult<Vec<String>> {
    use oxyde_migrate::{Dialect, MigrationOp};

    let ops: Vec<MigrationOp> = serde_json::from_str(operations_json)
        .map_err(|e| PyErr::new::<PyValueError, _>(format!("Failed to parse operations: {}", e)))?;

    let dialect_enum = match dialect {
        "sqlite" => Dialect::Sqlite,
        "postgres" => Dialect::Postgres,
        "mysql" => Dialect::Mysql,
        _ => {
            return Err(PyErr::new::<PyValueError, _>(format!(
                "Invalid dialect: {}",
                dialect
            )))
        }
    };

    let mut all_sql = Vec::new();
    for op in &ops {
        let sqls = op
            .to_sql(dialect_enum)
            .map_err(|e| PyErr::new::<PyRuntimeError, _>(format!("Migration error: {}", e)))?;
        all_sql.extend(sqls);
    }
    Ok(all_sql)
}

//! Python exception classes and `DriverError` → `PyErr` mapping.
//!
//! Constraint violations are classified by the driver (`DriverError::db_kind`,
//! backed by sqlx's cross-dialect `ErrorKind`) — never by matching error
//! message text. The Python package maps these onto its own exception
//! hierarchy in `oxyde.exceptions`.

use oxyde_driver::{DbErrorKind, DriverError};
use pyo3::exceptions::PyRuntimeError;
use pyo3::PyErr;

pyo3::create_exception!(
    _oxyde_core,
    DatabaseError,
    PyRuntimeError,
    "A database call failed."
);
pyo3::create_exception!(
    _oxyde_core,
    IntegrityError,
    DatabaseError,
    "A database constraint was violated."
);
pyo3::create_exception!(
    _oxyde_core,
    UniqueViolationError,
    IntegrityError,
    "A UNIQUE constraint was violated."
);
pyo3::create_exception!(
    _oxyde_core,
    ForeignKeyViolationError,
    IntegrityError,
    "A FOREIGN KEY constraint was violated."
);
pyo3::create_exception!(
    _oxyde_core,
    NotNullViolationError,
    IntegrityError,
    "A NOT NULL constraint was violated."
);
pyo3::create_exception!(
    _oxyde_core,
    CheckViolationError,
    IntegrityError,
    "A CHECK constraint was violated."
);

/// Convert a `DriverError` into the matching Python exception.
pub(crate) fn driver_err(e: &DriverError) -> PyErr {
    match e {
        DriverError::InvalidListenerChannels(_) => {
            return pyo3::exceptions::PyValueError::new_err(e.to_string())
        }
        DriverError::ListenerUnsupported => {
            return pyo3::exceptions::PyNotImplementedError::new_err(e.to_string())
        }
        _ => {}
    }
    let message = e.to_string();
    match e.db_kind() {
        Some(DbErrorKind::UniqueViolation) => UniqueViolationError::new_err(message),
        Some(DbErrorKind::ForeignKeyViolation) => ForeignKeyViolationError::new_err(message),
        Some(DbErrorKind::NotNullViolation) => NotNullViolationError::new_err(message),
        Some(DbErrorKind::CheckViolation) => CheckViolationError::new_err(message),
        Some(DbErrorKind::Other) => DatabaseError::new_err(message),
        None => PyErr::new::<PyRuntimeError, _>(message),
    }
}

//! Conversion helpers: backend/dialect mapping, pool settings extraction,
//! and value conversion between Rust types and Python objects.

use std::time::Duration;

use oxyde_driver::{DatabaseBackend, PoolSettings as DriverPoolSettings};
use oxyde_query::Dialect;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyList, PyString};
use sea_query::Value as QueryValue;
use serde_json::Value as JsonValue;

/// Map driver's `DatabaseBackend` to query's `Dialect`.
pub(crate) fn backend_to_dialect(backend: DatabaseBackend) -> Dialect {
    match backend {
        DatabaseBackend::Postgres => Dialect::Postgres,
        DatabaseBackend::MySql => Dialect::Mysql,
        DatabaseBackend::Sqlite => Dialect::Sqlite,
    }
}

/// Extract `PoolSettings` from a Python dict or object with `to_payload()` method.
pub(crate) fn extract_pool_settings(
    _py: Python<'_>,
    settings: Option<Bound<'_, PyAny>>,
) -> PyResult<DriverPoolSettings> {
    let mut parsed = DriverPoolSettings::default();

    let Some(obj) = settings else {
        return Ok(parsed);
    };

    if obj.is_none() {
        return Ok(parsed);
    }

    if let Ok(dict) = obj.downcast::<PyDict>() {
        parse_pool_dict(dict, &mut parsed)?;
        return Ok(parsed);
    }

    if obj.hasattr("to_payload")? {
        let payload = obj.call_method0("to_payload")?;
        if payload.is_none() {
            return Ok(parsed);
        }
        let dict = payload.downcast::<PyDict>()?;
        parse_pool_dict(dict, &mut parsed)?;
        return Ok(parsed);
    }

    let type_name = obj.get_type().name()?.to_string();
    Err(PyErr::new::<PyTypeError, _>(format!(
        "Pool settings must be a dict or expose to_payload(), got {}",
        type_name
    )))
}

/// Convert sea_query `Value` slice to a Python list.
pub(crate) fn values_to_py<'py>(
    py: Python<'py>,
    values: &[QueryValue],
) -> PyResult<Bound<'py, PyAny>> {
    let list = PyList::empty(py);
    for value in values {
        list.append(value_to_py(py, value))?;
    }
    Ok(list.into_any())
}

/// Convert a single sea_query `Value` to a Python object.
#[allow(unreachable_patterns)]
pub(crate) fn value_to_py(py: Python<'_>, value: &QueryValue) -> Py<PyAny> {
    match value {
        QueryValue::Bool(Some(v)) => PyBool::new(py, *v).to_owned().unbind().into_any(),
        QueryValue::Bool(None) => py.None(),
        QueryValue::TinyInt(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::TinyInt(None) => py.None(),
        QueryValue::SmallInt(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::SmallInt(None) => py.None(),
        QueryValue::Int(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::Int(None) => py.None(),
        QueryValue::BigInt(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::BigInt(None) => py.None(),
        QueryValue::TinyUnsigned(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::TinyUnsigned(None) => py.None(),
        QueryValue::SmallUnsigned(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::SmallUnsigned(None) => py.None(),
        QueryValue::Unsigned(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::Unsigned(None) => py.None(),
        QueryValue::BigUnsigned(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::BigUnsigned(None) => py.None(),
        QueryValue::Float(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::Float(None) => py.None(),
        QueryValue::Double(Some(v)) => (*v).into_pyobject(py).unwrap().unbind().into_any(),
        QueryValue::Double(None) => py.None(),
        QueryValue::String(Some(s)) => PyString::new(py, s.as_str()).unbind().into_any(),
        QueryValue::String(None) => py.None(),
        QueryValue::Char(Some(c)) => {
            let text = c.to_string();
            PyString::new(py, &text).unbind().into_any()
        }
        QueryValue::Char(None) => py.None(),
        QueryValue::Bytes(Some(bytes)) => PyBytes::new(py, bytes.as_slice()).unbind().into_any(),
        QueryValue::Bytes(None) => py.None(),
        _ => PyString::new(py, &format!("{:?}", value))
            .unbind()
            .into_any(),
    }
}

/// Recursively convert `serde_json::Value` to a Python object.
pub(crate) fn json_to_py(py: Python<'_>, value: &JsonValue) -> PyResult<Py<PyAny>> {
    Ok(match value {
        JsonValue::Null => py.None(),
        JsonValue::Bool(v) => PyBool::new(py, *v).to_owned().unbind().into_any(),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_pyobject(py).unwrap().unbind().into_any()
            } else if let Some(u) = n.as_u64() {
                u.into_pyobject(py).unwrap().unbind().into_any()
            } else if let Some(f) = n.as_f64() {
                f.into_pyobject(py).unwrap().unbind().into_any()
            } else {
                py.None()
            }
        }
        JsonValue::String(s) => PyString::new(py, s).unbind().into_any(),
        JsonValue::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(json_to_py(py, item)?)?;
            }
            list.unbind().into_any()
        }
        JsonValue::Object(map) => {
            let dict = PyDict::new(py);
            for (key, val) in map {
                dict.set_item(key, json_to_py(py, val)?)?;
            }
            dict.unbind().into_any()
        }
    })
}

fn parse_pool_dict(dict: &Bound<'_, PyDict>, parsed: &mut DriverPoolSettings) -> PyResult<()> {
    if let Some(value) = dict.get_item("max_connections")? {
        parsed.max_connections = extract_optional_u32(&value)?;
    }
    if let Some(value) = dict.get_item("min_connections")? {
        parsed.min_connections = extract_optional_u32(&value)?;
    }
    if let Some(value) = dict.get_item("connect_timeout")? {
        parsed.acquire_timeout = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("idle_timeout")? {
        parsed.idle_timeout = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("acquire_timeout")? {
        parsed.acquire_timeout = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("max_lifetime")? {
        parsed.max_lifetime = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("test_before_acquire")? {
        parsed.test_before_acquire = extract_optional_bool(&value)?;
    }
    // Extract transaction cleanup settings
    if let Some(value) = dict.get_item("transaction_timeout")? {
        parsed.transaction_timeout = extract_optional_duration(&value)?;
    }
    if let Some(value) = dict.get_item("transaction_cleanup_interval")? {
        parsed.transaction_cleanup_interval = extract_optional_duration(&value)?;
    }

    // Extract SQLite PRAGMA settings
    if let Some(value) = dict.get_item("sqlite_journal_mode")? {
        parsed.sqlite_journal_mode = extract_optional_string(&value)?;
    }
    if let Some(value) = dict.get_item("sqlite_synchronous")? {
        parsed.sqlite_synchronous = extract_optional_string(&value)?;
    }
    if let Some(value) = dict.get_item("sqlite_cache_size")? {
        parsed.sqlite_cache_size = extract_optional_i32(&value)?;
    }
    if let Some(value) = dict.get_item("sqlite_busy_timeout")? {
        parsed.sqlite_busy_timeout = extract_optional_i32(&value)?;
    }

    Ok(())
}

fn extract_optional_u32(value: &Bound<'_, PyAny>) -> PyResult<Option<u32>> {
    if value.is_none() {
        Ok(None)
    } else {
        value
            .extract::<u32>()
            .map(Some)
            .map_err(|e| PyErr::new::<PyTypeError, _>(e.to_string()))
    }
}

fn extract_optional_i32(value: &Bound<'_, PyAny>) -> PyResult<Option<i32>> {
    if value.is_none() {
        Ok(None)
    } else {
        value
            .extract::<i32>()
            .map(Some)
            .map_err(|e| PyErr::new::<PyTypeError, _>(e.to_string()))
    }
}

fn extract_optional_string(value: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
    if value.is_none() {
        Ok(None)
    } else {
        value
            .extract::<String>()
            .map(Some)
            .map_err(|e| PyErr::new::<PyTypeError, _>(e.to_string()))
    }
}

fn extract_optional_bool(value: &Bound<'_, PyAny>) -> PyResult<Option<bool>> {
    if value.is_none() {
        Ok(None)
    } else {
        value
            .extract::<bool>()
            .map(Some)
            .map_err(|e| PyErr::new::<PyTypeError, _>(e.to_string()))
    }
}

fn extract_optional_duration(value: &Bound<'_, PyAny>) -> PyResult<Option<Duration>> {
    if value.is_none() {
        return Ok(None);
    }

    if let Ok(seconds) = value.extract::<f64>() {
        return Ok(Some(Duration::from_secs_f64(seconds)));
    }

    if let Ok(seconds) = value.extract::<u64>() {
        return Ok(Some(Duration::from_secs(seconds)));
    }

    if value.hasattr("total_seconds")? {
        let seconds_obj = value.call_method0("total_seconds")?;
        let seconds = seconds_obj.extract::<f64>()?;
        return Ok(Some(Duration::from_secs_f64(seconds)));
    }

    Err(PyErr::new::<PyTypeError, _>(
        "Duration must be provided as seconds (float/int) or datetime.timedelta".to_string(),
    ))
}

#[cfg(all(test, not(feature = "extension-module")))]
mod tests {
    use super::*;
    use pyo3::types::PyDict;

    #[test]
    fn test_extract_pool_settings_from_dict() {
        pyo3::prepare_freethreaded_python();
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("max_connections", 20u32).unwrap();
            dict.set_item("min_connections", 5u32).unwrap();
            dict.set_item("connect_timeout", 1.5f64).unwrap();
            dict.set_item("idle_timeout", 2.0f64).unwrap();
            dict.set_item("acquire_timeout", 3.0f64).unwrap();
            dict.set_item("max_lifetime", 4.0f64).unwrap();
            dict.set_item("test_before_acquire", true).unwrap();

            let settings = extract_pool_settings(py, Some(dict.into_any())).unwrap();
            assert_eq!(settings.max_connections, Some(20));
            assert_eq!(settings.min_connections, Some(5));
            assert!((settings.acquire_timeout.unwrap().as_secs_f64() - 3.0).abs() < f64::EPSILON);
            assert!((settings.idle_timeout.unwrap().as_secs_f64() - 2.0).abs() < f64::EPSILON);
            assert!((settings.max_lifetime.unwrap().as_secs_f64() - 4.0).abs() < f64::EPSILON);
            assert_eq!(settings.test_before_acquire, Some(true));
        });
    }

    #[test]
    fn test_extract_pool_settings_rejects_invalid_type() {
        pyo3::prepare_freethreaded_python();
        Python::attach(|py| {
            let value = "invalid".into_pyobject(py).unwrap().into_any();
            let err = extract_pool_settings(py, Some(value)).unwrap_err();
            assert!(err.to_string().contains("Pool settings must be a dict"));
        });
    }
}

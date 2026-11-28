use std::sync::Arc;

use oxyde_driver::NotificationListener;
use pyo3::prelude::*;

#[pyclass]
struct ListenerHandle {
    listener: Arc<NotificationListener>,
}

#[pymethods]
impl ListenerHandle {
    fn recv<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let listener = Arc::clone(&self.listener);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let notification = listener
                .recv()
                .await
                .map_err(|e| crate::errors::driver_err(&e))?;
            Ok(notification.map(|n| {
                (
                    n.channel().to_owned(),
                    n.payload().to_owned(),
                    n.process_id(),
                )
            }))
        })
    }

    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.listener.request_close();
        let listener = Arc::clone(&self.listener);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            listener.close().await;
            Ok(())
        })
    }
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        self.listener.request_close();
    }
}

#[pyfunction]
pub(crate) fn listen(
    py: Python<'_>,
    pool_name: String,
    channels: Vec<String>,
) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let listener = NotificationListener::connect(&pool_name, &channels)
            .await
            .map_err(|e| crate::errors::driver_err(&e))?;
        Ok(ListenerHandle {
            listener: Arc::new(listener),
        })
    })
}

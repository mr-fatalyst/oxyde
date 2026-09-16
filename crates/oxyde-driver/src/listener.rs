use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::{PgListener, PgNotification, PgPoolOptions};
use tokio::sync::{watch, Mutex};

use crate::{registry, DbPool, DriverError, Result};

pub struct NotificationListener {
    listener: Arc<Mutex<Option<PgListener>>>,
    stop: watch::Sender<bool>,
    done: watch::Receiver<bool>,
}

impl NotificationListener {
    pub async fn connect(name: &str, channels: &[String]) -> Result<Self> {
        validate_channels(channels)?;
        let DbPool::Postgres(application_pool) = registry().get(name).await?.clone_pool() else {
            return Err(DriverError::ListenerUnsupported);
        };

        let listener_pool = application_pool
            .close_event()
            .do_until(
                PgPoolOptions::new()
                    .max_connections(1)
                    .min_connections(0)
                    .max_lifetime(None)
                    .idle_timeout(None)
                    .connect_with((*application_pool.connect_options()).clone()),
            )
            .await
            .and_then(std::convert::identity)
            .map_err(|e| DriverError::db("acquiring notification listener", e))?;
        let mut listener = application_pool
            .close_event()
            .do_until(PgListener::connect_with(&listener_pool))
            .await
            .and_then(std::convert::identity)
            .map_err(|e| DriverError::db("acquiring notification listener", e))?;
        application_pool
            .close_event()
            .do_until(listener.listen_all(channels.iter().map(String::as_str)))
            .await
            .and_then(std::convert::identity)
            .map_err(|e| DriverError::db("subscribing notification listener", e))?;

        let listener = Arc::new(Mutex::new(Some(listener)));
        let (stop, mut stopped) = watch::channel(false);
        let (finished, done) = watch::channel(false);
        let owned = Arc::clone(&listener);
        let shutdown = stop.clone();
        tokio::spawn(async move {
            tokio::select! {
                biased;
                () = application_pool.close_event() => {},
                _ = stopped.wait_for(|value| *value) => {},
            }
            shutdown.send_replace(true);
            drop(owned.lock().await.take());
            let _ = tokio::time::timeout(Duration::from_secs(1), listener_pool.close()).await;
            finished.send_replace(true);
        });
        Ok(Self {
            listener,
            stop,
            done,
        })
    }

    pub async fn recv(&self) -> Result<Option<PgNotification>> {
        let mut stopped = self.stop.subscribe();
        if *stopped.borrow() {
            return Ok(None);
        }
        let mut guard = tokio::select! {
            biased;
            _ = stopped.wait_for(|value| *value) => return Ok(None),
            guard = self.listener.lock() => guard,
        };
        let Some(listener) = guard.as_mut() else {
            return Ok(None);
        };
        tokio::select! {
            biased;
            _ = stopped.wait_for(|value| *value) => Ok(None),
            result = listener.recv() => match result {
                Ok(notification) => Ok(Some(notification)),
                Err(error) => {
                    self.request_close();
                    Err(DriverError::db("receiving notification", error))
                }
            },
        }
    }

    pub fn request_close(&self) {
        self.stop.send_replace(true);
    }

    pub async fn close(&self) {
        self.request_close();
        let mut done = self.done.clone();
        let _ = done.wait_for(|value| *value).await;
    }
}

impl Drop for NotificationListener {
    fn drop(&mut self) {
        self.request_close();
    }
}

fn validate_channels(channels: &[String]) -> Result<()> {
    if channels.is_empty() {
        return Err(DriverError::InvalidListenerChannels(
            "at least one channel is required".into(),
        ));
    }
    let mut seen = HashSet::new();
    for channel in channels {
        if channel.is_empty() || channel.contains('\0') || channel.len() > 63 {
            return Err(DriverError::InvalidListenerChannels(
                "channel names must contain 1–63 UTF-8 bytes and no NUL".into(),
            ));
        }
        if !seen.insert(channel) {
            return Err(DriverError::InvalidListenerChannels(
                "duplicate channel names are not allowed".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_channels;

    #[test]
    fn channel_validation() {
        for channels in [
            vec![],
            vec![String::new()],
            vec!["a\0b".into()],
            vec!["a".repeat(64)],
            vec!["é".repeat(32)],
            vec!["a".into(), "a".into()],
        ] {
            assert!(validate_channels(&channels).is_err());
        }
        assert!(validate_channels(&["a".repeat(63), "é".repeat(31), "a\"b".into()]).is_ok());
    }
}

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{net::SocketAddr, str::FromStr};

use provider_relay_server::{
    app,
    identity::cleanup::delete_expired_identities,
    storage::{MasterKey, Storage, StorageError},
    AppState,
};

const STARTUP_CLEANUP_FAILURE: &str = "startup identity cleanup failed";

fn log_cleanup_failure(error: &StorageError) {
    tracing::warn!(
        error_code = error.code().as_str(),
        "failed to delete inactive identities"
    );
}

fn handle_startup_cleanup_failure(error: StorageError) -> anyhow::Error {
    tracing::error!(
        error_code = error.code().as_str(),
        failure_kind = "startup_identity_cleanup",
        "failed to delete inactive identities at startup"
    );
    anyhow::anyhow!(STARTUP_CLEANUP_FAILURE)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "provider_relay_server=info,tower_http=info".into()),
        )
        .init();

    std::fs::create_dir_all("data")?;
    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(
            SqliteConnectOptions::from_str("sqlite:data/relay.db?mode=rwc")?.foreign_keys(true),
        )
        .await?;
    let storage = Storage::initialize(db.clone(), MasterKey::from_environment()?).await?;
    let deleted = delete_expired_identities(&storage)
        .await
        .map_err(handle_startup_cleanup_failure)?;
    if deleted > 0 {
        tracing::info!(deleted, "deleted inactive identities at startup");
    }
    let cleanup_storage = storage.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            match delete_expired_identities(&cleanup_storage).await {
                Ok(deleted) if deleted > 0 => {
                    tracing::info!(deleted, "deleted inactive identities");
                }
                Ok(_) => {}
                Err(error) => log_cleanup_failure(&error),
            }
        }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let state = AppState {
        db,
        storage,
        client,
    };
    let app = app::router(state).await?;

    let port: u16 = std::env::var("LISTEN_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(18080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!(%addr, "Provider Relay server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fmt::{self, Write as _},
        sync::{Arc, Mutex},
    };

    use provider_relay_server::storage::StorageError;
    use tracing::{
        field::{Field, Visit},
        Event, Subscriber,
    };
    use tracing_subscriber::{layer::Context, prelude::*, Layer, Registry};

    #[derive(Clone)]
    struct EventCapture {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl<S> Layer<S> for EventCapture
    where
        S: Subscriber,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut visitor = FieldVisitor(String::new());
            event.record(&mut visitor);
            self.events
                .lock()
                .expect("lock captured tracing events")
                .push(format!("{} {}", event.metadata().name(), visitor.0));
        }
    }

    struct FieldVisitor(String);

    impl Visit for FieldVisitor {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            let _ = write!(self.0, "{}={value:?};", field.name());
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            let _ = write!(self.0, "{}={value};", field.name());
        }
    }

    #[test]
    fn cleanup_failure_log_excludes_storage_error_details() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = Registry::default().with(EventCapture {
            events: Arc::clone(&events),
        });
        let error = StorageError::Database(sqlx::Error::Protocol(
            "SELECT provider_key, device_credential FROM identities".to_string(),
        ));

        tracing::subscriber::with_default(subscriber, || {
            super::log_cleanup_failure(&error);
        });

        let events = events.lock().expect("lock captured tracing events");
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("error_code=internal"));
        assert!(!events[0].contains("SELECT provider_key"));
        assert!(!events[0].contains("device_credential"));
    }

    #[test]
    fn startup_cleanup_failure_uses_safe_error_and_log() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = Registry::default().with(EventCapture {
            events: Arc::clone(&events),
        });
        let error = StorageError::Database(sqlx::Error::Protocol(
            "SELECT provider_key WHERE device_credential = 'device-secret'".to_string(),
        ));

        let startup_error = tracing::subscriber::with_default(subscriber, || {
            super::handle_startup_cleanup_failure(error)
        });

        let returned = format!("{startup_error:#}");
        assert_eq!(returned, "startup identity cleanup failed");
        assert!(!returned.contains("provider_key"));
        assert!(!returned.contains("device_credential"));
        assert!(!returned.contains("device-secret"));

        let events = events.lock().expect("lock captured tracing events");
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("error_code=internal"));
        assert!(events[0].contains("failure_kind=startup_identity_cleanup"));
        assert!(events[0].contains("failed to delete inactive identities at startup"));
        assert!(!events[0].contains("provider_key"));
        assert!(!events[0].contains("device_credential"));
        assert!(!events[0].contains("device-secret"));
    }
}

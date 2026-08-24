use std::{net::SocketAddr, path::Path};

use prelay_server::{
    app,
    client_update::ClientUpdateCache,
    database::{connect, DatabaseConfig},
    identity::cleanup::delete_expired_identities,
    schema::initialize,
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

fn load_environment_file(path: &Path) -> anyhow::Result<()> {
    match dotenvy::from_path(path) {
        Ok(()) => Ok(()),
        Err(error) if error.not_found() => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_environment_file(Path::new(".env"))?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "prelay_server=info,tower_http=info".into()),
        )
        .init();

    let database_config = DatabaseConfig::from_environment()?;
    let db = connect(&database_config).await?;
    initialize(&db).await?;
    let storage = Storage::from_connection(db, MasterKey::from_environment()?);
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

    let upstream_policy =
        prelay_server::upstream::initialize_from_environment().map_err(anyhow::Error::msg)?;
    let client = reqwest::Client::builder()
        .timeout(upstream_policy.timeout)
        .build()?;
    let client_update = ClientUpdateCache::from_environment(client.clone()).await?;
    if let Err(error) = client_update.refresh().await {
        tracing::warn!(error = %error, "failed to refresh client update cache at startup");
    }
    let refresh_client_update = client_update.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(6 * 60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = refresh_client_update.refresh().await {
                tracing::warn!(error = %error, "failed to refresh client update cache");
            }
        }
    });
    let state = AppState {
        storage,
        client,
        client_update,
    };
    let app = app::router(state).await?;

    let port: u16 = std::env::var("LISTEN_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(18080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!(%addr, "Prelay server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fmt::{self, Write as _},
        path::PathBuf,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    use prelay_server::{entity::identities, schema::initialize, storage::StorageError};
    use sea_orm::{Database, EntityTrait};
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

    fn assert_excludes_storage_error_details(value: &str) {
        let normalized = value.to_ascii_lowercase();
        for detail in [
            "sql",
            "database",
            "db",
            "crypto",
            "provider_key",
            "device_credential",
            "secret",
        ] {
            assert!(
                !normalized.contains(detail),
                "value unexpectedly exposed {detail}: {value}"
            );
        }
    }

    #[tokio::test]
    async fn service_startup_initializes_an_empty_database() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect to in-memory SQLite");

        initialize(&db).await.expect("initialize service database");
        identities::Entity::find()
            .all(&db)
            .await
            .expect("identities table exists");
    }

    #[test]
    fn cleanup_failure_log_excludes_storage_error_details() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = Registry::default().with(EventCapture {
            events: Arc::clone(&events),
        });
        let error = StorageError::Database(sea_orm::DbErr::Custom(
            "SQL database DB crypto provider_key device_credential secret".to_string(),
        ));

        tracing::subscriber::with_default(subscriber, || {
            super::log_cleanup_failure(&error);
        });

        let events = events.lock().expect("lock captured tracing events");
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("error_code=internal"));
        assert_excludes_storage_error_details(&events[0]);
    }

    #[test]
    fn startup_cleanup_failure_uses_safe_error_and_log() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = Registry::default().with(EventCapture {
            events: Arc::clone(&events),
        });
        let error = StorageError::Database(sea_orm::DbErr::Custom(
            "SQL database DB crypto provider_key device_credential secret".to_string(),
        ));

        let startup_error = tracing::subscriber::with_default(subscriber, || {
            super::handle_startup_cleanup_failure(error)
        });

        let returned = format!("{startup_error:#}");
        assert_eq!(returned, "startup identity cleanup failed");
        assert_excludes_storage_error_details(&returned);

        let events = events.lock().expect("lock captured tracing events");
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("error_code=internal"));
        assert!(events[0].contains("failure_kind=startup_identity_cleanup"));
        assert!(events[0].contains("failed to delete inactive identities at startup"));
        assert_excludes_storage_error_details(&events[0]);
    }

    #[test]
    fn environment_file_loads_values_without_overriding_process_environment() {
        const KEY: &str = "PRELAY_DOTENV_TEST_VALUE";
        let _restore = EnvironmentVariableRestore::capture(KEY);
        let path = temporary_environment_file();

        std::fs::write(&path, format!("{KEY}=from-file\n")).expect("write environment file");
        std::env::remove_var(KEY);
        super::load_environment_file(&path).expect("load environment file");
        assert_eq!(std::env::var(KEY).as_deref(), Ok("from-file"));

        std::env::set_var(KEY, "from-process");
        super::load_environment_file(&path).expect("reload environment file");
        assert_eq!(std::env::var(KEY).as_deref(), Ok("from-process"));

        std::fs::remove_file(path).expect("remove environment file");
    }

    #[test]
    fn missing_environment_file_does_not_prevent_startup() {
        let path = temporary_environment_file();

        super::load_environment_file(&path).expect("ignore a missing environment file");
    }

    fn temporary_environment_file() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("prelay-server-{unique}.env"))
    }

    struct EnvironmentVariableRestore {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvironmentVariableRestore {
        fn capture(key: &'static str) -> Self {
            Self {
                key,
                original: std::env::var_os(key),
            }
        }
    }

    impl Drop for EnvironmentVariableRestore {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

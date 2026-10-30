use std::{
    ffi::OsString,
    fmt::{self, Write as _},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{store::entity::identities, store::schema::initialize, store::storage::StorageError};
use sea_orm::EntityTrait;
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, Layer, Registry};

use super::diagnostics::{handle_startup_cleanup_failure, log_cleanup_failure};
use super::environment::{listen_address, load_environment_file};

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
    let db = crate::test_support::test_database_connection().await;

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
        log_cleanup_failure(&error);
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

    let startup_error =
        tracing::subscriber::with_default(subscriber, || handle_startup_cleanup_failure(error));

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
    load_environment_file(&path).expect("load environment file");
    assert_eq!(std::env::var(KEY).as_deref(), Ok("from-file"));

    std::env::set_var(KEY, "from-process");
    load_environment_file(&path).expect("reload environment file");
    assert_eq!(std::env::var(KEY).as_deref(), Ok("from-process"));

    std::fs::remove_file(path).expect("remove environment file");
}

#[test]
fn missing_environment_file_does_not_prevent_startup() {
    let path = temporary_environment_file();

    load_environment_file(&path).expect("ignore a missing environment file");
}

#[test]
fn listen_address_defaults_and_accepts_a_complete_socket_address() {
    assert_eq!(
        listen_address(None).expect("use the default listen address"),
        "0.0.0.0:18080".parse().unwrap(),
    );
    assert_eq!(
        listen_address(Some("127.0.0.1:19090")).expect("parse the configured listen address"),
        "127.0.0.1:19090".parse().unwrap(),
    );
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

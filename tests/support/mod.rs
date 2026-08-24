use std::{
    ops::Deref,
    sync::{Arc, Mutex, OnceLock},
};

use prelay_server::{
    database::{connect, DatabaseConfig, DatabaseKind},
    schema::initialize,
    storage::{MasterKey, Storage},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

const SQLITE_TEST_DATABASE_URL: &str = "sqlite::memory:";

pub struct TestStorage {
    storage: Storage,
    _postgres_serial_guard: Option<OwnedMutexGuard<()>>,
}

impl TestStorage {
    pub fn storage(&self) -> &Storage {
        &self.storage
    }
}

impl Deref for TestStorage {
    type Target = Storage;

    fn deref(&self) -> &Self::Target {
        self.storage()
    }
}

#[test]
fn explicit_postgres_test_url_is_selected_over_sqlite_default() {
    let _lock = test_database_url_environment_lock()
        .lock()
        .expect("lock test database URL environment");
    let original = std::env::var_os("TEST_POSTGRES_URL");
    std::env::set_var(
        "TEST_POSTGRES_URL",
        "postgres://prelay_test:prelay_test@127.0.0.1:5432/prelay_test",
    );

    assert_eq!(
        DatabaseConfig::from_url(&test_database_url_while_locked())
            .expect("valid PostgreSQL test database URL")
            .kind(),
        DatabaseKind::Postgres
    );

    match original {
        Some(value) => std::env::set_var("TEST_POSTGRES_URL", value),
        None => std::env::remove_var("TEST_POSTGRES_URL"),
    }
}

pub async fn test_storage() -> TestStorage {
    let config = DatabaseConfig::from_url(&test_database_url()).expect("valid test database URL");
    let postgres_serial_guard = if config.kind() == DatabaseKind::Postgres {
        Some(postgres_test_database_lock().lock_owned().await)
    } else {
        None
    };
    let connection = connect(&config).await.expect("connect test database");

    initialize(&connection)
        .await
        .expect("initialize test database schema");
    TestStorage {
        storage: Storage::from_connection(connection, MasterKey::from_bytes([0; 32])),
        _postgres_serial_guard: postgres_serial_guard,
    }
}

fn test_database_url() -> String {
    let _lock = test_database_url_environment_lock()
        .lock()
        .expect("lock test database URL environment");
    test_database_url_while_locked()
}

fn test_database_url_while_locked() -> String {
    std::env::var("TEST_POSTGRES_URL").unwrap_or_else(|_| SQLITE_TEST_DATABASE_URL.to_owned())
}

fn test_database_url_environment_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn postgres_test_database_lock() -> Arc<AsyncMutex<()>> {
    static LOCK: OnceLock<Arc<AsyncMutex<()>>> = OnceLock::new();
    Arc::clone(LOCK.get_or_init(|| Arc::new(AsyncMutex::new(()))))
}

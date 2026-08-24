use prelay_server::{
    database::{connect, DatabaseConfig, DatabaseConfigError},
    schema::initialize,
};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use std::sync::{Mutex, OnceLock};

fn environment_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn set_environment_variable(name: &str, value: Option<&str>) {
    match value {
        Some(value) => unsafe { std::env::set_var(name, value) },
        None => unsafe { std::env::remove_var(name) },
    }
}

#[test]
fn rejects_missing_or_unsupported_database_url() {
    assert!(matches!(
        DatabaseConfig::from_url(""),
        Err(DatabaseConfigError::MissingUrl)
    ));
    assert!(matches!(
        DatabaseConfig::from_url("mysql://localhost/prelay"),
        Err(DatabaseConfigError::UnsupportedScheme { .. })
    ));
}

#[test]
fn sqlite_is_single_connection_and_postgres_uses_configured_limit() {
    assert_eq!(
        DatabaseConfig::from_url("sqlite::memory:")
            .expect("SQLite URL is supported")
            .max_connections(),
        1
    );
    assert_eq!(
        DatabaseConfig::from_url("postgres://user:pass@host/prelay")
            .expect("PostgreSQL URL is supported")
            .max_connections(),
        10
    );
}

#[test]
fn reads_database_url_from_environment() {
    let _guard = environment_lock().lock().expect("lock environment");
    set_environment_variable("DATABASE_URL", Some("postgres://localhost/prelay"));
    set_environment_variable("DATABASE_MAX_CONNECTIONS", None);

    let config = DatabaseConfig::from_environment().expect("read PostgreSQL configuration");

    assert_eq!(
        config.kind(),
        prelay_server::database::DatabaseKind::Postgres
    );
    assert_eq!(config.max_connections(), 10);
    set_environment_variable("DATABASE_URL", None);
}

#[test]
fn rejects_missing_or_invalid_postgres_connection_limit_from_environment() {
    let _guard = environment_lock().lock().expect("lock environment");
    set_environment_variable("DATABASE_URL", None);
    set_environment_variable("DATABASE_MAX_CONNECTIONS", None);
    assert!(matches!(
        DatabaseConfig::from_environment(),
        Err(DatabaseConfigError::MissingUrl)
    ));

    set_environment_variable("DATABASE_URL", Some("postgres://localhost/prelay"));
    set_environment_variable("DATABASE_MAX_CONNECTIONS", Some("0"));
    assert!(matches!(
        DatabaseConfig::from_environment(),
        Err(DatabaseConfigError::InvalidMaxConnections { .. })
    ));

    set_environment_variable("DATABASE_MAX_CONNECTIONS", Some("not-a-number"));
    assert!(matches!(
        DatabaseConfig::from_environment(),
        Err(DatabaseConfigError::InvalidMaxConnections { .. })
    ));
    set_environment_variable("DATABASE_URL", None);
    set_environment_variable("DATABASE_MAX_CONNECTIONS", None);
}

#[test]
fn ignores_connection_limit_override_for_sqlite() {
    let _guard = environment_lock().lock().expect("lock environment");
    set_environment_variable("DATABASE_URL", Some("sqlite::memory:"));
    set_environment_variable("DATABASE_MAX_CONNECTIONS", Some("not-a-number"));

    let config = DatabaseConfig::from_environment().expect("read SQLite configuration");

    assert_eq!(config.max_connections(), 1);
    set_environment_variable("DATABASE_URL", None);
    set_environment_variable("DATABASE_MAX_CONNECTIONS", None);
}

#[tokio::test]
async fn connection_errors_do_not_expose_database_credentials() {
    let config = DatabaseConfig::from_url("postgres://user:secret@127.0.0.1:1/prelay")
        .expect("PostgreSQL URL is supported");
    let error = connect(&config)
        .await
        .expect_err("connection to an unused local port must fail");

    let displayed = error.to_string();
    let debugged = format!("{error:?}");
    assert!(!displayed.contains("secret"));
    assert!(!debugged.contains("secret"));
}

#[tokio::test]
async fn connects_to_a_supported_sqlite_database() {
    let config = DatabaseConfig::from_url("sqlite::memory:").expect("SQLite URL is supported");
    let connection = connect(&config).await.expect("connect to in-memory SQLite");

    connection.ping().await.expect("ping SQLite connection");
}

#[tokio::test]
async fn initializes_an_empty_database_without_migration_history() {
    let config = DatabaseConfig::from_url("sqlite::memory:").expect("SQLite URL is supported");
    let connection = connect(&config).await.expect("connect to in-memory SQLite");

    initialize(&connection)
        .await
        .expect("initialize the current schema");
    let migration_history = connection
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'seaql_migrations'"
                .to_owned(),
        ))
        .await
        .expect("inspect SQLite schema")
        .expect("migration history count");
    assert_eq!(migration_history.try_get::<i64>("", "COUNT(*)").unwrap(), 0);
    initialize(&connection)
        .await
        .expect("reuse the initialized schema");
}

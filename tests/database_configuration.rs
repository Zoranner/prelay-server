use prelay_server::{
    database::{connect, DatabaseConfig, DatabaseConfigError},
    schema::initialize,
    test_support::{test_database_config, test_database_connection},
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
    assert!(matches!(
        DatabaseConfig::from_url("sqlite::memory:"),
        Err(DatabaseConfigError::UnsupportedScheme { .. })
    ));
}

#[test]
fn postgres_uses_the_configured_connection_limit() {
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
async fn connects_to_the_configured_test_database() {
    let config = test_database_config();
    let connection = connect(&config)
        .await
        .expect("connect to the test PostgreSQL database");

    connection.ping().await.expect("ping the test database");
}

#[tokio::test]
async fn initializes_a_database_without_migration_history() {
    let connection = test_database_connection().await;

    initialize(&connection)
        .await
        .expect("reuse the initialized schema");
    let migration_history = connection
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT COUNT(*) AS history_count FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = 'seaql_migrations'"
                .to_owned(),
        ))
        .await
        .expect("inspect the schema")
        .expect("migration history count");
    assert_eq!(
        migration_history
            .try_get::<i64>("", "history_count")
            .unwrap(),
        0
    );
}

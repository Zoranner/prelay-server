use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement,
};

use crate::{
    domain::catalog::ProviderCatalog,
    outbound::integrations::client_update::ClientUpdateCache,
    outbound::integrations::extensions::ExtensionCatalog,
    store::connect::DatabaseConfig,
    store::schema::initialize,
    store::storage::{MasterKey, Storage},
    AppState,
};

/// 测试库里的 schema 前缀：每个用例一个 schema，互不干扰。
const TEST_SCHEMA_PREFIX: &str = "prelay_test_";
/// 只清理一小时前的测试 schema，避免误删正在运行的用例。
const STALE_TEST_SCHEMA_MILLIS: u128 = 60 * 60 * 1000;

static TEST_SCHEMA_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// `TEST_POSTGRES_URL` 必须指向专用测试库：用例会在库里创建 `prelay_test_*` schema。
pub fn test_database_url() -> String {
    let url = std::env::var("TEST_POSTGRES_URL").unwrap_or_else(|_| {
        panic!(
            "TEST_POSTGRES_URL must point at a dedicated PostgreSQL test database; \
             the server no longer supports SQLite"
        )
    });
    // 测试会执行建表、加列和删表；关闭 SQLx 预备语句缓存，避免 PostgreSQL
    // 在 ALTER TABLE 后复用到旧的 result type（cached plan must not change result type）。
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}statement-cache-capacity=0")
}

pub fn test_database_config() -> DatabaseConfig {
    DatabaseConfig::from_url(&test_database_url())
        .expect("TEST_POSTGRES_URL must be a valid PostgreSQL URL")
}

// 测试使用固定目录，避免用例断言随 config/catalog 变更而失效。
pub fn fixture_catalog() -> ProviderCatalog {
    ProviderCatalog::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/catalog")
            .as_path(),
    )
    .expect("load fixture provider catalog")
}

/// 独立 schema 里的已初始化连接：用例之间不共享任何数据。
pub async fn test_database_connection() -> DatabaseConnection {
    let db = test_empty_database_connection().await;
    initialize(&db)
        .await
        .expect("initialize test database schema");
    db
}

/// 独立 schema 里的空连接：供 schema 初始化测试自行建表。
pub async fn test_empty_database_connection() -> DatabaseConnection {
    let config = test_database_config();
    let admin = crate::store::connect::connect(&config)
        .await
        .expect("connect to the test PostgreSQL database");
    let schema = format!(
        "{TEST_SCHEMA_PREFIX}{}_{}_{}",
        now_millis(),
        std::process::id(),
        TEST_SCHEMA_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    admin
        .execute_unprepared(&format!("CREATE SCHEMA \"{schema}\""))
        .await
        .expect("create the test schema");
    let _ = drop_stale_test_schemas(&admin).await;

    let mut options = ConnectOptions::new(config.url().to_owned());
    options
        .max_connections(config.max_connections())
        .sqlx_logging(false)
        .set_schema_search_path(schema);
    let db = Database::connect(options)
        .await
        .expect("connect to the test schema");
    db
}

pub async fn test_storage() -> Storage {
    let db = test_database_connection().await;
    Storage::from_connection(db, MasterKey::from_bytes([0; 32]))
}

pub async fn test_state() -> AppState {
    let storage = test_storage().await;
    let activity_store = crate::service::activity::ActivityStore::new(
        storage.clone(),
        crate::store::rollout::RolloutStore::new(
            std::env::temp_dir().join(format!("prelay-rollout-test-{}", uuid::Uuid::new_v4())),
        ),
    );
    AppState {
        provider_catalog: std::sync::Arc::new(fixture_catalog()),
        storage,
        activity_store,
        client: reqwest::Client::new(),
        provider_clients: Default::default(),
        client_update: ClientUpdateCache::unavailable(reqwest::Client::new()),
        extensions: ExtensionCatalog::unavailable(reqwest::Client::new()),
        failover: Default::default(),
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default()
}

/// 尽力清理遗留 schema；失败不影响用例。
async fn drop_stale_test_schemas(admin: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    let cutoff = now_millis().saturating_sub(STALE_TEST_SCHEMA_MILLIS);
    let rows = admin
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            format!("SELECT nspname FROM pg_namespace WHERE nspname LIKE '{TEST_SCHEMA_PREFIX}%'"),
        ))
        .await?;
    for row in rows {
        let name: String = row.try_get("", "nspname")?;
        let stale = name
            .strip_prefix(TEST_SCHEMA_PREFIX)
            .and_then(|rest| rest.split('_').next())
            .and_then(|stamp| stamp.parse::<u128>().ok())
            .is_some_and(|stamp| stamp < cutoff);
        if stale {
            admin
                .execute_unprepared(&format!("DROP SCHEMA IF EXISTS \"{name}\" CASCADE"))
                .await?;
        }
    }
    Ok(())
}

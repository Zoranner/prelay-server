pub mod activity;
pub mod app;
pub mod bridge;
pub mod client_update;
pub mod database;
pub mod entity;
pub mod error;
pub mod extensions;
pub mod identity;
pub mod memory;
pub mod models;
pub mod observability;
pub mod provider_catalog;
pub mod providers;
pub mod routes;
pub mod schema;
pub mod stats;
pub mod storage;
pub mod upstream;

use error::AppError;

#[derive(Clone)]
pub struct AppState {
    pub provider_catalog: std::sync::Arc<provider_catalog::ProviderCatalog>,
    pub storage: storage::Storage,
    pub client: reqwest::Client,
    pub provider_clients: upstream::ProviderClientCache,
    pub client_update: client_update::ClientUpdateCache,
    pub extensions: extensions::ExtensionCatalog,
}

impl AppState {
    /// 供应商的上游客户端：目录条目配了代理就用带代理的，没配就用直连客户端。
    pub fn provider_client(&self, provider_type: &str) -> Result<reqwest::Client, AppError> {
        let proxy_url = self.provider_catalog.provider_proxy_url(provider_type);
        let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(self.client.clone());
        };
        self.provider_clients
            .client(proxy_url, upstream::policy())
            .map_err(|error| AppError::Upstream {
                status: None,
                message: format!("供应商代理地址不可用 {proxy_url}: {error}"),
            })
    }
}

pub mod test_support {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use sea_orm::{
        ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement,
    };

    use crate::{
        client_update::ClientUpdateCache,
        database::DatabaseConfig,
        extensions::ExtensionCatalog,
        provider_catalog::ProviderCatalog,
        schema::initialize,
        storage::{MasterKey, Storage},
        AppState,
    };

    /// 测试库里的 schema 前缀：每个用例一个 schema，互不干扰。
    const TEST_SCHEMA_PREFIX: &str = "prelay_test_";
    /// 只清理一小时前的测试 schema，避免误删正在运行的用例。
    const STALE_TEST_SCHEMA_MILLIS: u128 = 60 * 60 * 1000;

    static TEST_SCHEMA_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    /// `TEST_POSTGRES_URL` 必须指向专用测试库：用例会在库里创建 `prelay_test_*` schema。
    pub fn test_database_url() -> String {
        std::env::var("TEST_POSTGRES_URL").unwrap_or_else(|_| {
            panic!(
                "TEST_POSTGRES_URL must point at a dedicated PostgreSQL test database; \
                 the server no longer supports SQLite"
            )
        })
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
        let admin = crate::database::connect(&config)
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
        AppState {
            provider_catalog: std::sync::Arc::new(fixture_catalog()),
            storage: test_storage().await,
            client: reqwest::Client::new(),
            provider_clients: Default::default(),
            client_update: ClientUpdateCache::unavailable(reqwest::Client::new()),
            extensions: ExtensionCatalog::unavailable(reqwest::Client::new()),
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
                format!(
                    "SELECT nspname FROM pg_namespace WHERE nspname LIKE '{TEST_SCHEMA_PREFIX}%'"
                ),
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
}

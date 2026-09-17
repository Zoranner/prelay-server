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
    use crate::{
        client_update::ClientUpdateCache,
        database::{connect, DatabaseConfig},
        extensions::ExtensionCatalog,
        provider_catalog::ProviderCatalog,
        schema::initialize,
        storage::{MasterKey, Storage},
        AppState,
    };

    // 测试使用固定目录，避免用例断言随 config/catalog 变更而失效。
    pub fn fixture_catalog() -> ProviderCatalog {
        ProviderCatalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/catalog")
                .as_path(),
        )
        .expect("load fixture provider catalog")
    }

    pub async fn test_state() -> AppState {
        let database_config =
            DatabaseConfig::from_url("sqlite::memory:").expect("valid in-memory SQLite URL");
        let db = connect(&database_config)
            .await
            .expect("connect to in-memory SQLite");
        initialize(&db)
            .await
            .expect("initialize test database schema");
        let storage = Storage::from_connection(db, MasterKey::from_bytes([0; 32]));

        AppState {
            provider_catalog: std::sync::Arc::new(fixture_catalog()),
            storage,
            client: reqwest::Client::new(),
            provider_clients: Default::default(),
            client_update: ClientUpdateCache::unavailable(reqwest::Client::new()),
            extensions: ExtensionCatalog::unavailable(reqwest::Client::new()),
        }
    }
}

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

#[derive(Clone)]
pub struct AppState {
    pub provider_catalog: std::sync::Arc<provider_catalog::ProviderCatalog>,
    pub storage: storage::Storage,
    pub client: reqwest::Client,
    pub client_update: client_update::ClientUpdateCache,
    pub extensions: extensions::ExtensionCatalog,
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
        let provider_catalog = std::sync::Arc::new(
            ProviderCatalog::load(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("deploy/app/config/catalog")
                    .as_path(),
            )
            .expect("load test provider catalog"),
        );

        AppState {
            provider_catalog,
            storage,
            client: reqwest::Client::new(),
            client_update: ClientUpdateCache::unavailable(reqwest::Client::new()),
            extensions: ExtensionCatalog::unavailable(reqwest::Client::new()),
        }
    }
}

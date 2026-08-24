pub mod app;
pub mod bridge;
pub mod database;
pub mod entity;
pub mod error;
pub mod identity;
pub mod models;
pub mod observability;
pub mod providers;
pub mod routes;
pub mod schema;
pub mod stats;
pub mod storage;
pub mod upstream;

#[derive(Clone)]
pub struct AppState {
    pub storage: storage::Storage,
    pub client: reqwest::Client,
}

pub mod test_support {
    use crate::{
        database::{connect, DatabaseConfig},
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

        AppState {
            storage,
            client: reqwest::Client::new(),
        }
    }
}

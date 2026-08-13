pub mod app;
pub mod bridge;
pub mod db;
pub mod error;
pub mod identity;
pub mod models;
pub mod observability;
pub mod providers;
pub mod routes;
pub mod stats;
pub mod storage;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub storage: storage::Storage,
    pub client: reqwest::Client,
}

pub mod test_support {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    use crate::{
        db,
        storage::{MasterKey, Storage},
        AppState,
    };

    pub async fn test_state() -> AppState {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::from_str("sqlite::memory:")
                    .expect("valid in-memory SQLite URL")
                    .foreign_keys(true),
            )
            .await
            .expect("create sqlite pool");
        db::init_schema(&db)
            .await
            .expect("initialize legacy schema");
        let storage = Storage::initialize(db.clone(), MasterKey::from_bytes([0; 32]))
            .await
            .expect("initialize identity storage");

        AppState {
            storage,
            db,
            client: reqwest::Client::new(),
        }
    }
}

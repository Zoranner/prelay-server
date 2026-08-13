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
    use sqlx::sqlite::SqlitePoolOptions;

    use crate::{db, storage::MasterKey, storage::Storage, AppState};

    pub async fn test_state() -> AppState {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("initialize schema");

        AppState {
            storage: Storage::from_pool(db.clone(), MasterKey::from_bytes([0; 32])),
            db,
            client: reqwest::Client::new(),
        }
    }
}

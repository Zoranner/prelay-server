pub mod app;
pub mod bridge;
pub mod db;
pub mod error;
pub mod models;
pub mod observability;
pub mod providers;
pub mod routes;
pub mod stats;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub client: reqwest::Client,
}

pub mod test_support {
    use sqlx::sqlite::SqlitePoolOptions;

    use crate::{db, AppState};

    pub async fn test_state() -> AppState {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("initialize schema");

        AppState {
            db,
            client: reqwest::Client::new(),
        }
    }
}

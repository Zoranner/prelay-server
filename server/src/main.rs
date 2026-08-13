use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;

use provider_relay_server::{app, db, storage::MasterKey, storage::Storage, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "provider_relay_server=info,tower_http=info".into()),
        )
        .init();

    std::fs::create_dir_all("data")?;
    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite:data/relay.db?mode=rwc")
        .await?;
    db::init_schema(&db).await?;
    let storage = Storage::initialize(db.clone(), MasterKey::from_environment()?).await?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let state = AppState {
        db,
        storage,
        client,
    };
    let app = app::router(state).await?;

    let port: u16 = std::env::var("LISTEN_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(18080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!(%addr, "Provider Relay server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

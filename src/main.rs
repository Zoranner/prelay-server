use axum::{routing::any, Router};
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, services::ServeDir};

mod bridge;
mod db;
mod error;
mod models;
mod providers;
mod routes;
mod stats;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub client: reqwest::Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "provider_relay=info,tower_http=info".into()),
        )
        .init();

    // Ensure data directory exists
    std::fs::create_dir_all("data")?;

    // Initialize SQLite connection pool
    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite:data/relay.db?mode=rwc")
        .await?;
    db::init_schema(&db).await?;

    // Build HTTP client with reasonable timeouts
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300)) // 5 min for long-running LLM calls
        .build()?;

    let state = AppState { db, client };

    // Proxy routes: handle all HTTP methods on /proxy/* and /proxy
    let proxy_router = Router::new()
        .route("/", any(routes::proxy::handle))
        .route("/*path", any(routes::proxy::handle))
        .with_state(state.clone());

    // Admin API routes
    let admin_router = routes::admin::router()
        .merge(routes::stats::router())
        .with_state(state.clone());

    let app = Router::new()
        .nest("/api", admin_router)
        .merge(routes::chat::router().with_state(state.clone()))
        .merge(routes::messages::router().with_state(state.clone()))
        .merge(routes::models::router().with_state(state.clone()))
        .merge(routes::responses::router().with_state(state.clone()))
        .nest("/proxy", proxy_router)
        // Serve built Vue frontend from ./static/
        .fallback_service(ServeDir::new("static").append_index_html_on_directories(true))
        .layer(CorsLayer::permissive());

    let port: u16 = std::env::var("LISTEN_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!("Provider Relay listening on http://{}", addr);
    tracing::info!("Admin UI: http://{}", addr);
    tracing::info!("Proxy endpoint: http://{}/proxy", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

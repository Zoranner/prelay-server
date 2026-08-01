use axum::{middleware, routing::any, Router};
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
};

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
    pub admin_token: Option<String>,
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

    let admin_token = configured_admin_token();
    if admin_token.is_none() {
        tracing::warn!("ADMIN_TOKEN 未配置，/api 管理接口将以兼容模式开放");
    }

    let state = AppState {
        db,
        client,
        admin_token,
    };

    // Proxy routes: handle all HTTP methods on /proxy/* and /proxy
    let proxy_router = Router::new()
        .route("/", any(routes::proxy::handle))
        .route("/*path", any(routes::proxy::handle))
        .with_state(state.clone());

    // Admin API routes
    let admin_router = routes::admin::router()
        .merge(routes::stats::router())
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            routes::auth::require_admin_auth,
        ));
    let protocol_router = routes::chat::router()
        .merge(routes::messages::router())
        .merge(routes::models::router())
        .merge(routes::responses::router())
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            routes::auth::require_protocol_auth,
        ));

    let app = Router::new()
        .merge(protocol_router)
        .nest("/api", admin_router)
        .nest("/proxy", proxy_router)
        // Serve built Vue frontend from ./static/
        .fallback_service(
            ServeDir::new("static")
                .append_index_html_on_directories(true)
                .fallback(ServeFile::new("static/index.html")),
        )
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

fn configured_admin_token() -> Option<String> {
    std::env::var("ADMIN_TOKEN")
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

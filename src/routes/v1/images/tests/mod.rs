use std::{
    convert::Infallible,
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use super::{handler::create_image_generation, router};
use crate::{
    models::ProviderCapabilityOverrides,
    routes::v1::endpoint_resolver::{
        create_test_endpoint_auth, create_test_endpoint_auth_with_candidates, test_provider,
        test_provider_with_capabilities,
    },
    AppState,
};
use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, HeaderValue, Response, StatusCode},
    middleware,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use bytes::Bytes;
use futures::StreamExt;
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::Notify};

#[derive(Clone)]
struct UpstreamState {
    hits: Arc<AtomicUsize>,
    payloads: Arc<Mutex<Vec<Value>>>,
    status: StatusCode,
    body: Bytes,
    content_type: HeaderValue,
    request_id: Option<HeaderValue>,
}

struct UpstreamFixture {
    url: String,
    hits: Arc<AtomicUsize>,
    payloads: Arc<Mutex<Vec<Value>>>,
}

async fn test_state() -> AppState {
    crate::test_support::test_state().await
}

async fn test_state_with_connection() -> (AppState, DatabaseConnection) {
    let config = crate::database::DatabaseConfig::from_url("sqlite::memory:")
        .expect("valid in-memory SQLite URL");
    let connection = crate::database::connect(&config)
        .await
        .expect("connect to in-memory SQLite");
    crate::schema::initialize(&connection)
        .await
        .expect("initialize test database schema");
    let state = AppState {
        provider_catalog: std::sync::Arc::new(
            crate::provider_catalog::ProviderCatalog::load(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("config/catalog")
                    .as_path(),
            )
            .expect("load provider catalog"),
        ),
        storage: crate::storage::Storage::from_connection(
            connection.clone(),
            crate::storage::MasterKey::from_bytes([0; 32]),
        ),
        client: reqwest::Client::new(),
        client_update: crate::client_update::ClientUpdateCache::unavailable(reqwest::Client::new()),
        extensions: crate::extensions::ExtensionCatalog::unavailable(reqwest::Client::new()),
    };
    (state, connection)
}

async fn reject_activity_inserts(connection: &DatabaseConnection) {
    connection
            .execute_unprepared(
                "CREATE TRIGGER reject_activity_inserts BEFORE INSERT ON identity_activities BEGIN SELECT RAISE(FAIL, 'forced activity failure'); END",
            )
            .await
            .expect("create activity failure trigger");
}

fn image_capabilities() -> ProviderCapabilityOverrides {
    ProviderCapabilityOverrides {
        upstream_protocols: Some(vec!["images_generations".to_string()]),
        ..ProviderCapabilityOverrides::default()
    }
}

mod activities;
mod candidates;
mod fixtures;
mod routing;
mod streaming;

use fixtures::*;

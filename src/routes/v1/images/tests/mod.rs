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
    let connection = crate::test_support::test_database_connection().await;
    let state = AppState {
        provider_catalog: std::sync::Arc::new(crate::test_support::fixture_catalog()),
        storage: crate::storage::Storage::from_connection(
            connection.clone(),
            crate::storage::MasterKey::from_bytes([0; 32]),
        ),
        client: reqwest::Client::new(),
        provider_clients: Default::default(),
        client_update: crate::client_update::ClientUpdateCache::unavailable(reqwest::Client::new()),
        extensions: crate::extensions::ExtensionCatalog::unavailable(reqwest::Client::new()),
    };
    (state, connection)
}

/// 让活动插入必然失败，用于验证活动写入失败时的路由行为。
async fn reject_activity_inserts(connection: &DatabaseConnection) {
    connection
        .execute_unprepared(
            "CREATE OR REPLACE FUNCTION reject_activity_inserts() RETURNS trigger AS $$ \
             BEGIN RAISE EXCEPTION 'forced activity failure'; END $$ LANGUAGE plpgsql",
        )
        .await
        .expect("create activity failure function");
    connection
        .execute_unprepared(
            "CREATE TRIGGER reject_activity_inserts BEFORE INSERT ON identity_activities \
             FOR EACH ROW EXECUTE FUNCTION reject_activity_inserts()",
        )
        .await
        .expect("create activity failure trigger");
}

mod activities;
mod candidates;
mod fixtures;
mod routing;
mod streaming;

use fixtures::*;

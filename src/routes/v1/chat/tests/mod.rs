use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::json;
use std::{convert::Infallible, time::Duration};
use tokio::net::TcpListener;
use tower::ServiceExt;

use super::{handler::create_chat_completion, router};
use crate::{
    models::ProviderCapabilityOverrides,
    routes::v1::endpoint_resolver::{
        create_test_endpoint_auth, create_test_endpoint_auth_with_candidates, test_provider,
        test_provider_with_capabilities,
    },
    AppState,
};

async fn test_state() -> AppState {
    crate::test_support::test_state().await
}

mod activities;
mod auth;
mod candidates;
mod fixtures;
mod routing;
mod streaming;

use fixtures::*;

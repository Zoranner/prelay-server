use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware,
    routing::post,
    Json, Router,
};
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use tokio::net::TcpListener;
use tower::ServiceExt;

use super::{handler::create_message, router};
use crate::routes::v1::endpoint_resolver::{
    create_test_endpoint_auth, create_test_endpoint_auth_with_candidates, test_provider,
};

mod auth;
mod candidates;
mod fixtures;
mod request_logs;
mod routing;
mod streaming;
mod tools;

use fixtures::*;

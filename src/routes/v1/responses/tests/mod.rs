use axum::body::Body;
use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use bytes::Bytes;
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use std::{convert::Infallible, time::Duration};
use tokio::net::TcpListener;
use tower::ServiceExt;

use super::handler::create_response;
use crate::routes::v1::endpoint_resolver::{
    create_empty_test_endpoint_auth, create_test_endpoint_auth,
    create_test_endpoint_auth_with_candidates, test_provider,
};

include!("cases_a.rs");
include!("cases_b.rs");
include!("support.rs");

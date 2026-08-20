use axum::{middleware, Router};

use crate::AppState;

pub mod auth;
mod chat;
mod interface_resolver;
mod messages;
mod models;
mod responses;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(chat::router())
        .merge(messages::router())
        .merge(models::router())
        .merge(responses::router())
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state,
            auth::require_protocol_auth,
        ))
}

use axum::{middleware, routing::post, Router};

use crate::AppState;

pub mod auth;
mod identities;
mod interfaces;
mod providers;
mod stats;

pub fn router(state: AppState) -> Router {
    let authenticated = Router::new()
        .merge(providers::router())
        .merge(interfaces::router())
        .merge(stats::router())
        .route(
            "/identity/credential/rotate",
            post(identities::rotate_credential),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_device_credential,
        ));
    Router::new()
        .route("/identities", post(identities::create_identity))
        .merge(authenticated)
        .with_state(state)
}

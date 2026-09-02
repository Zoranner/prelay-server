use axum::{
    middleware,
    routing::{get, post},
    Router,
};

use crate::AppState;

pub mod auth;
mod client_update;
mod endpoints;
mod extensions;
mod identities;
mod provider_catalog;
mod providers;
mod stats;

pub fn router(state: AppState) -> Router {
    let authenticated = Router::new()
        .merge(providers::router())
        .merge(endpoints::router())
        .merge(provider_catalog::router())
        .merge(stats::router())
        .merge(client_update::router())
        .merge(extensions::router())
        .route(
            "/identity/credential/rotate",
            post(identities::rotate_credential),
        )
        .route("/identity", get(identities::current_identity))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_device_credential,
        ));
    Router::new()
        .route("/identities", post(identities::create_identity))
        .merge(authenticated)
        .with_state(state)
}

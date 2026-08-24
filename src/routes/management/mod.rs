use axum::{middleware, routing::post, Router};

use crate::AppState;

pub mod auth;
mod client_update;
mod endpoints;
mod identities;
mod providers;
mod stats;

pub fn router(state: AppState) -> Router {
    let authenticated = Router::new()
        .merge(providers::router())
        .merge(endpoints::router())
        .merge(stats::router())
        .merge(client_update::router())
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

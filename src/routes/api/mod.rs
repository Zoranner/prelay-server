use axum::{
    middleware,
    routing::{get, post},
    Router,
};

use crate::AppState;

pub mod auth;
mod catalog;
mod client_update;
mod endpoints;
mod error;
mod extensions;
mod identities;
mod providers;
mod stats;

pub use error::{ApiError, ApiJson, ApiQuery};

pub fn router(state: AppState) -> Router {
    let authenticated = Router::new()
        .merge(providers::router())
        .merge(endpoints::router())
        .merge(catalog::router())
        .merge(stats::router())
        .merge(client_update::router())
        .merge(extensions::router())
        .route(
            "/identity/credential/rotate",
            post(identities::rotate_credential),
        )
        .route("/identities", get(identities::directory))
        .route("/identity", get(identities::current_identity))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_device_credential,
        ));
    Router::new()
        .route("/identities", post(identities::create_identity))
        .merge(authenticated)
        .fallback(unknown_route)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
}

async fn unknown_route() -> ApiError {
    ApiError::not_found("management API route does not exist")
}

/// 路径存在但方法不匹配：管理面按"该路径没有这个方法的管理接口"处理，
/// 与未知路径使用同一个错误码，客户端只需按 not_found 提示。
async fn method_not_allowed() -> ApiError {
    ApiError::not_found("management API route does not exist for this request method")
}

use axum::{routing::post, Router};

use crate::AppState;

mod activity;
mod candidate;
mod chat;
mod handler;
pub(super) mod image_budget;
mod native;
mod sessions;
mod state;

#[cfg(test)]
mod tests;

pub fn router() -> Router<AppState> {
    Router::new().route("/responses", post(handler::create_response))
}

use axum::{routing::post, Router};

use crate::AppState;

mod anthropic;
mod candidate;
mod chat;
mod handler;
mod native;
mod sessions;

#[cfg(test)]
mod tests;

pub fn router() -> Router<AppState> {
    Router::new().route("/responses", post(handler::create_response))
}

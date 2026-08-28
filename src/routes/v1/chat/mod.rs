use axum::{routing::post, Router};

use crate::AppState;

mod candidate;
mod handler;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

pub fn router() -> Router<AppState> {
    Router::new().route("/chat/completions", post(handler::create_chat_completion))
}

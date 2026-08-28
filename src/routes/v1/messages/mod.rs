use axum::{routing::post, Router};

use crate::AppState;

mod candidate;
mod chat;
mod handler;
mod native;
mod responses;

#[cfg(test)]
mod tests;

pub fn router() -> Router<AppState> {
    Router::new().route("/messages", post(handler::create_message))
}

fn count_tool_calls(response: &crate::bridge::internal::InternalResponse) -> i64 {
    response
        .output
        .iter()
        .filter(|item| item.is_tool_call())
        .count() as i64
}

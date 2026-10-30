use axum::{routing::post, Router};

use crate::AppState;

const IMAGE_GENERATIONS_PROTOCOL: &str = "images_generations";

mod activity;
mod candidate;
mod handler;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/images/generations",
        post(handler::create_image_generation),
    )
}

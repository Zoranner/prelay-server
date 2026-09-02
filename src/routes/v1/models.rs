use axum::{
    extract::{Extension, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::collections::HashSet;

use crate::{
    error::AppError, models::EndpointModel, routes::v1::auth::CurrentProtocolAccess, AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/models", get(list_models))
}

#[derive(Debug, Serialize)]
struct ModelsResponse {
    object: &'static str,
    data: Vec<ModelEntry>,
}

#[derive(Debug, Serialize)]
struct ModelEntry {
    id: String,
    object: &'static str,
    created: i64,
    owned_by: &'static str,
}

async fn list_models(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
) -> Result<Json<ModelsResponse>, AppError> {
    let models = state
        .storage
        .list_protocol_models(&crate::storage::ProtocolAccess {
            identity_id: access.identity_id,
            endpoint_id: access.endpoint_id,
            endpoint_name: access.endpoint_name,
        })
        .await?;
    let mut model_ids = HashSet::new();
    let mut data: Vec<ModelEntry> = Vec::new();
    for model in models.into_iter().filter(|model| {
        state
            .provider_catalog
            .language_model(&model.model.upstream_model)
            .is_some()
            && state.provider_catalog.provider_supports_language_model(
                &model.provider.provider_type,
                &model.model.upstream_model,
            )
    }) {
        if model_ids.insert(model.model.model_name.clone()) {
            data.push(model_entry_for_endpoint_model(model.model));
        }
    }

    Ok(Json(ModelsResponse {
        object: "list",
        data,
    }))
}

fn model_entry_for_endpoint_model(model: EndpointModel) -> ModelEntry {
    ModelEntry {
        id: model.model_name,
        object: "model",
        created: 0,
        owned_by: "prelay",
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        extract::State,
        http::{Request, StatusCode},
        middleware, Router,
    };
    use tower::ServiceExt;

    use super::list_models;
    use crate::routes::v1::endpoint_resolver::{
        create_test_endpoint_auth, create_test_endpoint_auth_with_candidates, test_provider,
        test_provider_with_capabilities,
    };

    #[tokio::test]
    async fn lists_identity_endpoint_models_only() {
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "DeepSeek",
            "deepseek",
            "https://api.deepseek.com",
            "sk-test",
        )
        .await
        .expect("create legacy provider source");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "coder", "deepseek-v4-pro").await;

        let response = list_models(State(state), auth.access)
            .await
            .expect("list models");
        let model = response.0.data.first().expect("identity model listed");

        assert_eq!(response.0.object, "list");
        assert_eq!(response.0.data.len(), 1);
        assert_eq!(model.id, "coder");
        assert_eq!(model.object, "model");
        assert_eq!(model.created, 0);
        assert_eq!(model.owned_by, "prelay");
    }

    #[tokio::test]
    async fn lists_image_only_model_with_image_protocol_metadata() {
        let state = crate::test_support::test_state().await;
        let provider = test_provider_with_capabilities(
            "Image provider",
            "custom_image",
            "https://images.example/v1",
            "sk-test",
            None,
        )
        .await
        .expect("create image provider source");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image", "image-upstream").await;

        let response = list_models(State(state), auth.access)
            .await
            .expect("list image models");
        assert!(response.0.data.is_empty());
    }

    #[tokio::test]
    async fn merges_downstream_protocols_across_same_name_candidates() {
        let state = crate::test_support::test_state().await;
        let text_provider = test_provider(
            "Text provider",
            "deepseek",
            "https://text.example/v1",
            "sk-text",
        )
        .await
        .expect("create text provider source");
        let image_provider = test_provider_with_capabilities(
            "Image provider",
            "deepseek",
            "https://images.example/v1",
            "sk-image",
            None,
        )
        .await
        .expect("create image provider source");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[text_provider, image_provider],
            "shared-model",
            "deepseek-v4-pro",
        )
        .await;

        let response = list_models(State(state), auth.access)
            .await
            .expect("list merged models");
        let model = response.0.data.first().expect("merged model listed");

        assert_eq!(response.0.data.len(), 1);
        assert_eq!(model.id, "shared-model");
        assert_eq!(model.object, "model");
    }

    #[tokio::test]
    async fn rejects_unauthenticated_models_request() {
        let state = crate::test_support::test_state().await;
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state,
                    crate::routes::v1::auth::require_protocol_auth,
                )),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("route request");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

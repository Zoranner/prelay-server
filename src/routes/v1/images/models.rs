use std::collections::HashMap;

use axum::{
    extract::{Extension, State},
    Json,
};
use serde::Serialize;

use crate::{
    error::AppError,
    models::EndpointModel,
    providers::spec::{ProviderSpec, UpstreamProtocol},
    routes::v1::auth::CurrentProtocolAccess,
    AppState,
};

#[derive(Debug, Serialize)]
pub(super) struct ImageGenerationModelsResponse {
    object: &'static str,
    data: Vec<ImageGenerationModelEntry>,
}

#[derive(Debug, Serialize)]
pub(super) struct ImageGenerationModelEntry {
    id: String,
    object: &'static str,
    entry_type: &'static str,
    owned_by: &'static str,
    provider_id: String,
    provider_name: String,
    upstream_model: String,
}

pub(super) async fn list_image_generation_models(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
) -> Result<Json<ImageGenerationModelsResponse>, AppError> {
    let models = state
        .storage
        .list_protocol_models(&crate::storage::ProtocolAccess {
            identity_id: access.identity_id,
            endpoint_id: access.endpoint_id,
            endpoint_name: access.endpoint_name,
        })
        .await?;
    let mut model_indices = HashMap::new();
    let mut data = Vec::new();
    for model in models.into_iter().filter(|model| {
        state
            .provider_catalog
            .image_generation_model(&model.model.upstream_model)
            .is_some()
            && state
                .provider_catalog
                .provider_supports_image_generation_model(
                    &model.provider.provider_type,
                    &model.model.upstream_model,
                )
            && ProviderSpec::from_provider_config(&model.provider)
                .supported_protocols
                .contains(&UpstreamProtocol::ImageGenerations)
    }) {
        let model_name = model.model.model_name.clone();
        if model_indices.contains_key(&model_name) {
            continue;
        }
        model_indices.insert(model_name, data.len());
        data.push(image_generation_model_entry(
            model.model,
            &model.provider.name,
        ));
    }

    Ok(Json(ImageGenerationModelsResponse {
        object: "list",
        data,
    }))
}

fn image_generation_model_entry(
    model: EndpointModel,
    provider_name: &str,
) -> ImageGenerationModelEntry {
    ImageGenerationModelEntry {
        id: model.model_name,
        object: "model",
        entry_type: "image_generation_model",
        owned_by: "prelay",
        provider_id: model.provider_id,
        provider_name: provider_name.to_string(),
        upstream_model: model.upstream_model,
    }
}

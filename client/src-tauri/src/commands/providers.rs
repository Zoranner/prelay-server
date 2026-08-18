use provider_relay_protocol::{
    CreateProviderRequest, ProviderCapabilityOverrides, ProviderOperationResponse,
    ProviderResponse, TestProviderProtocolRequest, UpdateProviderRequest,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    api_client::ClientError,
    commands::{authenticated_api, OperationStatus},
    NativeState,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProviderSaveInput {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    pub capabilities: Option<ProviderCapabilityOverrides>,
    pub models: Vec<String>,
}

#[tauri::command]
pub async fn providers_list(
    state: State<'_, NativeState>,
) -> Result<Vec<ProviderResponse>, ClientError> {
    authenticated_api(&state).await?.get("/api/providers").await
}

#[tauri::command]
pub async fn providers_save(
    state: State<'_, NativeState>,
    provider_id: Option<String>,
    input: ProviderSaveInput,
) -> Result<ProviderResponse, ClientError> {
    let client = authenticated_api(&state).await?;
    match provider_id {
        Some(provider_id) => {
            let input = UpdateProviderRequest {
                name: Some(input.name),
                provider_type: Some(input.provider_type),
                base_url: Some(input.base_url),
                api_key: non_empty(input.api_key),
                capabilities: input.capabilities,
                models: Some(input.models),
            };
            client
                .patch(&format!("/api/providers/{provider_id}"), &input)
                .await
        }
        None => {
            let api_key = non_empty(input.api_key).ok_or_else(|| {
                ClientError::new(
                    "validation_failed",
                    "provider API key is required when creating",
                )
            })?;
            let input = CreateProviderRequest {
                name: input.name,
                provider_type: input.provider_type,
                base_url: input.base_url,
                api_key,
                capabilities: input.capabilities,
                models: input.models,
            };
            client.post("/api/providers", &input).await
        }
    }
}

#[tauri::command]
pub async fn providers_delete(
    state: State<'_, NativeState>,
    provider_id: String,
) -> Result<OperationStatus, ClientError> {
    authenticated_api(&state)
        .await?
        .delete(&format!("/api/providers/{provider_id}"))
        .await?;
    Ok(OperationStatus {
        message: "provider deleted".to_string(),
    })
}

#[tauri::command]
pub async fn providers_ping(
    state: State<'_, NativeState>,
    provider_id: String,
) -> Result<ProviderOperationResponse, ClientError> {
    authenticated_api(&state)
        .await?
        .post(
            &format!("/api/providers/{provider_id}/ping"),
            &serde_json::json!({}),
        )
        .await
}

#[tauri::command]
pub async fn providers_discover_models(
    state: State<'_, NativeState>,
    provider_id: String,
) -> Result<ProviderOperationResponse, ClientError> {
    authenticated_api(&state)
        .await?
        .post(
            &format!("/api/providers/{provider_id}/discover-models"),
            &serde_json::json!({}),
        )
        .await
}

#[tauri::command]
pub async fn providers_test_protocol(
    state: State<'_, NativeState>,
    provider_id: String,
    protocol: String,
    model: Option<String>,
) -> Result<ProviderOperationResponse, ClientError> {
    authenticated_api(&state)
        .await?
        .post(
            &format!("/api/providers/{provider_id}/test-protocol"),
            &TestProviderProtocolRequest { protocol, model },
        )
        .await
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

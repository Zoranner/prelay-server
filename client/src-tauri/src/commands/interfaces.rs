use provider_relay_protocol::{
    CreateInterfaceRequest, InterfaceModelInput, InterfaceResponse, UpdateInterfaceRequest,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    api_client::ClientError,
    commands::{authenticated_api, OperationStatus},
    NativeState,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InterfaceSaveInput {
    pub name: String,
    pub protocol: String,
    pub models: Vec<InterfaceModelInput>,
}

#[tauri::command]
pub async fn interfaces_list(
    state: State<'_, NativeState>,
) -> Result<Vec<InterfaceResponse>, ClientError> {
    authenticated_api(&state)
        .await?
        .get("/api/interfaces")
        .await
}

#[tauri::command]
pub async fn interfaces_save(
    state: State<'_, NativeState>,
    interface_id: Option<String>,
    input: InterfaceSaveInput,
) -> Result<InterfaceResponse, ClientError> {
    let client = authenticated_api(&state).await?;
    match interface_id {
        Some(interface_id) => {
            let input = UpdateInterfaceRequest {
                name: Some(input.name),
                protocol: Some(input.protocol),
                models: Some(input.models),
            };
            client
                .patch(&format!("/api/interfaces/{interface_id}"), &input)
                .await
        }
        None => {
            let input = CreateInterfaceRequest {
                name: input.name,
                protocol: Some(input.protocol),
                models: input.models,
            };
            client.post("/api/interfaces", &input).await
        }
    }
}

#[tauri::command]
pub async fn interfaces_delete(
    state: State<'_, NativeState>,
    interface_id: String,
) -> Result<OperationStatus, ClientError> {
    authenticated_api(&state)
        .await?
        .delete(&format!("/api/interfaces/{interface_id}"))
        .await?;
    Ok(OperationStatus {
        message: "interface deleted".to_string(),
    })
}

#[tauri::command]
pub async fn interfaces_regenerate_token(
    state: State<'_, NativeState>,
    interface_id: String,
) -> Result<InterfaceResponse, ClientError> {
    authenticated_api(&state)
        .await?
        .post(
            &format!("/api/interfaces/{interface_id}/regenerate-token"),
            &serde_json::json!({}),
        )
        .await
}

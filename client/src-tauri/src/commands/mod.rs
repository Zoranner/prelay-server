pub mod bootstrap;
pub mod interfaces;
pub mod providers;
pub mod stats;

use crate::{
    api_client::{ApiClient, ClientError},
    credential_store::CredentialStore,
    identity::IdentitySource,
    NativeState,
};

pub(crate) async fn authenticated_api(state: &NativeState) -> Result<ApiClient<'_>, ClientError> {
    let identity = state
        .identity
        .identity()
        .map_err(|error| ClientError::new("internal", error))?;
    let client = ApiClient::from_environment(&state.credentials)?;
    client.ensure_registered(&identity).await?;
    Ok(client)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OperationStatus {
    pub message: String,
}

#[tauri::command]
pub async fn credential_rotate(
    state: tauri::State<'_, NativeState>,
) -> Result<OperationStatus, ClientError> {
    let client = authenticated_api(&state).await?;
    let response: provider_relay_protocol::RotateCredentialResponse = client
        .post("/api/identity/credential/rotate", &serde_json::json!({}))
        .await?;
    state
        .credentials
        .save(&response.credential)
        .map_err(|error| ClientError::new("credential_store_error", error))?;
    Ok(OperationStatus {
        message: "device credential rotated".to_string(),
    })
}

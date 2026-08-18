pub mod bootstrap;
pub mod interfaces;
pub mod providers;
pub mod stats;

use crate::{
    api_client::{generate_device_credential, ApiClient, ClientError},
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
    client
        .ensure_registered_once(&identity, &state.registration_gate)
        .await?;
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
    let current_credential = state
        .credentials
        .load()
        .map_err(|error| ClientError::new("credential_store_error", error))?
        .filter(|record| !record.current.trim().is_empty())
        .ok_or_else(|| {
            ClientError::new(
                ClientError::MISSING_DEVICE_CREDENTIAL,
                "device credential is unavailable; identity cannot be restored automatically",
            )
        })?
        .current;
    let new_credential = generate_device_credential();
    state
        .credentials
        .begin_rotation(&new_credential)
        .map_err(|error| ClientError::new("credential_store_error", error))?;
    let response: provider_relay_protocol::RotateCredentialResponse = client
        .post_with_credential(
            "/api/identity/credential/rotate",
            &provider_relay_protocol::RotateCredentialRequest {
                new_credential: new_credential.clone(),
            },
            &current_credential,
        )
        .await?;
    debug_assert!(response.rotated);
    state
        .credentials
        .complete_rotation(&new_credential)
        .map_err(|error| ClientError::new("credential_store_error", error))?;
    Ok(OperationStatus {
        message: "device credential rotated".to_string(),
    })
}

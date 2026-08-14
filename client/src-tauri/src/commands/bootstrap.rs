use serde::Serialize;
use tauri::State;

use crate::{
    api_client::{ApiClient, ClientError},
    credential_store::CredentialStore,
    identity::{IdentitySource, WindowsIdentitySource},
    NativeState,
};

#[derive(Debug, Serialize)]
pub struct BootstrapResponse {
    pub relay_url: String,
    pub machine_id: String,
    pub account_sid: String,
    pub username: String,
    pub has_device_credential: bool,
}

pub fn collect_bootstrap(
    identity_source: &impl IdentitySource,
    credential_store: &impl CredentialStore,
) -> Result<BootstrapResponse, String> {
    let identity = identity_source.identity()?;
    let has_device_credential = credential_store.load()?.is_some();
    let relay_url = crate::api_client::configured_relay_url().map_err(|error| error.to_string())?;

    Ok(BootstrapResponse {
        relay_url,
        machine_id: identity.machine_id,
        account_sid: identity.account_sid,
        username: identity.username,
        has_device_credential,
    })
}

#[tauri::command]
pub async fn bootstrap(state: State<'_, NativeState>) -> Result<BootstrapResponse, ClientError> {
    let identity = state
        .identity
        .identity()
        .map_err(|error| ClientError::new("internal", error))?;
    let client = ApiClient::from_environment(&state.credentials)?;
    client.ensure_registered(&identity).await?;

    collect_bootstrap(&state.identity, &state.credentials)
        .map_err(|error| ClientError::new("internal", error))
}

pub fn native_state() -> NativeState {
    NativeState {
        identity: WindowsIdentitySource,
        credentials: crate::credential_store::WindowsCredentialStore,
    }
}

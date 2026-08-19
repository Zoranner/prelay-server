use serde::Serialize;
use tauri::State;

use crate::{
    api_client::{ApiClient, ClientError},
    identity::IdentitySource,
    NativeState,
};

#[derive(Debug, Serialize)]
pub struct BootstrapResponse {
    pub relay_url: String,
    pub username: String,
    pub has_device_credential: bool,
}

pub fn collect_bootstrap(
    identity_source: &impl IdentitySource,
    api_client: &ApiClient<'_>,
) -> Result<BootstrapResponse, ClientError> {
    let identity = identity_source
        .identity()
        .map_err(|error| ClientError::new("internal", error))?;
    let has_device_credential = api_client.has_stored_credential()?;
    let relay_url = api_client.base_url().to_owned();

    Ok(BootstrapResponse {
        relay_url,
        username: identity.username,
        has_device_credential,
    })
}

#[tauri::command]
pub async fn bootstrap(state: State<'_, NativeState>) -> Result<BootstrapResponse, ClientError> {
    let api_client = crate::commands::authenticated_api(&state).await?;
    collect_bootstrap(&state.identity, &api_client)
}

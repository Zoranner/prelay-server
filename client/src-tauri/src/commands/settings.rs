use serde::Serialize;
use tauri::State;

use crate::{
    api_client::{normalize_relay_url, ClientError},
    relay_settings::RelaySettingsStore,
    NativeState,
};

#[derive(Debug, Serialize)]
pub struct RelaySettingsResponse {
    pub relay_url: Option<String>,
}

#[tauri::command]
pub fn relay_settings_get(
    state: State<'_, NativeState>,
) -> Result<RelaySettingsResponse, ClientError> {
    let relay_url = state
        .relay_settings
        .load()
        .map_err(|error| ClientError::new("relay_settings_error", error))?;
    Ok(RelaySettingsResponse { relay_url })
}

#[tauri::command]
pub fn relay_settings_save(
    relay_url: String,
    state: State<'_, NativeState>,
) -> Result<RelaySettingsResponse, ClientError> {
    let relay_url = normalize_relay_url(&relay_url)?;
    state
        .relay_settings
        .save(&relay_url)
        .map_err(|error| ClientError::new("relay_settings_error", error))?;
    Ok(RelaySettingsResponse {
        relay_url: Some(relay_url),
    })
}

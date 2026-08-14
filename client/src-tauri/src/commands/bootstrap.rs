use serde::Serialize;
use tauri::State;

use crate::{
    credential_store::CredentialStore,
    identity::{IdentitySource, WindowsIdentitySource},
    NativeState,
};

#[derive(Debug, Serialize)]
pub struct BootstrapResponse {
    pub machine_id: String,
    pub account_sid: String,
    pub username: String,
    pub has_device_credential: bool,
}

pub fn bootstrap(
    identity_source: &impl IdentitySource,
    credential_store: &impl CredentialStore,
) -> Result<BootstrapResponse, String> {
    let identity = identity_source.identity()?;
    let has_device_credential = credential_store.load()?.is_some();

    Ok(BootstrapResponse {
        machine_id: identity.machine_id,
        account_sid: identity.account_sid,
        username: identity.username,
        has_device_credential,
    })
}

#[tauri::command]
pub fn bootstrap_client(state: State<'_, NativeState>) -> Result<BootstrapResponse, String> {
    bootstrap(&state.identity, &state.credentials)
}

pub fn native_state() -> NativeState {
    NativeState {
        identity: WindowsIdentitySource,
        credentials: crate::credential_store::WindowsCredentialStore,
    }
}

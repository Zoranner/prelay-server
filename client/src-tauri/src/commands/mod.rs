pub mod bootstrap;
pub mod interfaces;
pub mod providers;
pub mod stats;

use std::future::Future;

use crate::{
    api_client::{generate_device_credential, ApiClient, ClientError},
    credential_store::CredentialStore,
    identity::IdentitySource,
    NativeState,
};

pub(crate) async fn authenticated_api(state: &NativeState) -> Result<ApiClient<'_>, ClientError> {
    let _guard = state.credential_lifecycle_gate.lock().await;
    authenticated_api_unlocked(state).await
}

async fn authenticated_api_unlocked(state: &NativeState) -> Result<ApiClient<'_>, ClientError> {
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
    run_credential_lifecycle(&state, credential_rotate_unlocked(&state)).await
}

async fn credential_rotate_unlocked(state: &NativeState) -> Result<OperationStatus, ClientError> {
    let client = authenticated_api_unlocked(state).await?;
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

async fn run_credential_lifecycle<T>(state: &NativeState, lifecycle: impl Future<Output = T>) -> T {
    let _guard = state.credential_lifecycle_gate.lock().await;
    lifecycle.await
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::sync::{oneshot, Notify};

    use super::run_credential_lifecycle;
    use crate::NativeState;

    #[test]
    fn second_rotation_waits_for_the_first_rotation_to_complete() {
        tauri::async_runtime::block_on(async {
            let state = Arc::new(NativeState::default());
            let events = Arc::new(Mutex::new(Vec::new()));
            let first_registered = Arc::new(Notify::new());
            let allow_first_completion = Arc::new(Notify::new());
            let second_attempted = Arc::new(Notify::new());
            let (second_registered, mut second_registered_rx) = oneshot::channel();

            let first = tauri::async_runtime::spawn({
                let state = state.clone();
                let events = events.clone();
                let first_registered = first_registered.clone();
                let allow_first_completion = allow_first_completion.clone();
                async move {
                    run_credential_lifecycle(&state, async move {
                        events.lock().unwrap().push("first registered");
                        first_registered.notify_one();
                        allow_first_completion.notified().await;
                        events.lock().unwrap().push("first completed");
                    })
                    .await;
                }
            });
            first_registered.notified().await;

            let second = tauri::async_runtime::spawn({
                let state = state.clone();
                let events = events.clone();
                let second_attempted = second_attempted.clone();
                async move {
                    second_attempted.notify_one();
                    run_credential_lifecycle(&state, async move {
                        events.lock().unwrap().push("second registered");
                        second_registered.send(()).unwrap();
                    })
                    .await;
                }
            });
            second_attempted.notified().await;
            tokio::task::yield_now().await;

            assert!(second_registered_rx.try_recv().is_err());
            allow_first_completion.notify_one();
            first.await.unwrap();
            second.await.unwrap();

            assert_eq!(
                *events.lock().unwrap(),
                vec!["first registered", "first completed", "second registered"]
            );
        });
    }
}

pub mod bootstrap;
pub mod interfaces;
pub mod providers;
pub mod settings;
pub mod stats;

use std::ops::Deref;

use crate::{
    api_client::{generate_device_credential, ApiClient, ClientError},
    credential_store::{CredentialStore, FileCredentialLifecycleGuard},
    identity::{IdentitySource, WindowsIdentity},
    relay_settings::RelaySettingsStore,
    NativeState,
};

pub(crate) struct AuthenticatedApi<'a> {
    _credential_lifecycle_guard: tokio::sync::MutexGuard<'a, ()>,
    _file_credential_lifecycle_guard: FileCredentialLifecycleGuard,
    client: ApiClient<'a>,
}

impl<'a> Deref for AuthenticatedApi<'a> {
    type Target = ApiClient<'a>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

pub(crate) async fn authenticated_api(
    state: &NativeState,
) -> Result<AuthenticatedApi<'_>, ClientError> {
    let identity = state
        .identity
        .identity()
        .map_err(|error| ClientError::new("internal", error))?;
    let relay_url = state
        .relay_settings
        .load()
        .map_err(|error| ClientError::new("relay_settings_error", error))?
        .ok_or_else(|| {
            ClientError::new("relay_url_not_configured", "relay URL is not configured")
        })?;
    authenticated_api_with_identity(state, &identity, &relay_url).await
}

async fn authenticated_api_with_identity<'a>(
    state: &'a NativeState,
    identity: &WindowsIdentity,
    relay_url: &str,
) -> Result<AuthenticatedApi<'a>, ClientError> {
    let credential_lifecycle_guard = state.credential_lifecycle_gate.lock().await;
    let file_credential_lifecycle_guard = state
        .credentials
        .acquire_lifecycle_lock()
        .await
        .map_err(|error| ClientError::new("credential_store_error", error))?;
    let client = ApiClient::new(relay_url, &state.credentials)?;
    client
        .ensure_registered_once(identity, &state.registration_gate)
        .await?;
    Ok(AuthenticatedApi {
        _credential_lifecycle_guard: credential_lifecycle_guard,
        _file_credential_lifecycle_guard: file_credential_lifecycle_guard,
        client,
    })
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

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{mpsc, Arc},
    };

    use tokio::sync::{oneshot, Notify};

    use super::authenticated_api_with_identity;
    use crate::{identity::WindowsIdentity, NativeState};
    use tempfile::tempdir;

    #[test]
    fn authenticated_api_holds_the_file_lifecycle_lock_until_the_client_is_dropped() {
        tauri::async_runtime::block_on(async {
            let directory = tempdir().unwrap();
            let first_state = Arc::new(NativeState::for_app_data_dir(directory.path().into()));
            let second_state = Arc::new(NativeState::for_app_data_dir(directory.path().into()));
            let identity = WindowsIdentity {
                machine_id: "machine-a".into(),
                account_sid: "S-1-5-21-100".into(),
                username: "Ada".into(),
            };
            let (base_url, registration_events, server) = registration_server();
            let (first_ready, first_ready_rx) = oneshot::channel();
            let release_first = Arc::new(Notify::new());

            let first = tauri::async_runtime::spawn({
                let state = first_state.clone();
                let identity = identity.clone();
                let base_url = base_url.clone();
                let release_first = release_first.clone();
                async move {
                    let api = authenticated_api_with_identity(&state, &identity, &base_url)
                        .await
                        .unwrap();
                    first_ready.send(()).unwrap();
                    release_first.notified().await;
                    drop(api);
                }
            });
            first_ready_rx.await.unwrap();
            assert_eq!(registration_events.recv().unwrap(), "registration");

            let second = tauri::async_runtime::spawn({
                let state = second_state.clone();
                let identity = identity.clone();
                let base_url = base_url.clone();
                async move {
                    let _api = authenticated_api_with_identity(&state, &identity, &base_url)
                        .await
                        .unwrap();
                }
            });
            tokio::task::yield_now().await;

            assert!(matches!(
                registration_events.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            release_first.notify_one();
            first.await.unwrap();
            second.await.unwrap();
            assert_eq!(registration_events.recv().unwrap(), "registration");
            server.join().unwrap();
        });
    }

    fn registration_server() -> (
        String,
        mpsc::Receiver<&'static str>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (events_tx, events_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_http_request(&mut stream);
                assert!(request.starts_with("POST /api/identities HTTP/1.1"));
                events_tx.send("registration").unwrap();
                let body = r#"{"identity_id":"identity-a","created":false}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (format!("http://{address}"), events_rx, server)
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        let header_end = loop {
            let read = stream.read(&mut chunk).unwrap();
            assert_ne!(read, 0);
            request.extend_from_slice(&chunk[..read]);
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = std::str::from_utf8(&request[..header_end]).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .or_else(|| {
                headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
            })
            .unwrap()
            .parse::<usize>()
            .unwrap();
        while request.len() < header_end + content_length {
            let read = stream.read(&mut chunk).unwrap();
            assert_ne!(read, 0);
            request.extend_from_slice(&chunk[..read]);
        }
        String::from_utf8(request).unwrap()
    }
}

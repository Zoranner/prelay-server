use provider_relay_client::{
    api_client::{ApiClient, ClientError, RegistrationGate},
    credential_store::{CredentialStore, MemoryCredentialStore},
    identity::WindowsIdentity,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    },
    thread,
    time::{Duration, Instant},
};

#[test]
fn management_request_reads_credential_store_and_uses_bearer_authorization() {
    let store = MemoryCredentialStore::with_secret("device-secret");
    let client = ApiClient::new("https://relay.example.test/", &store).expect("create client");

    let request = client
        .authenticated_request("GET", "/api/providers")
        .expect("build authenticated request");

    assert_eq!(request.url(), "https://relay.example.test/api/providers");
    assert_eq!(request.authorization(), "Bearer device-secret");
}

#[test]
fn management_request_without_credential_returns_stable_error_code() {
    let store = MemoryCredentialStore::default();
    let client = ApiClient::new("https://relay.example.test", &store).expect("create client");

    let error = client
        .authenticated_request("GET", "/api/providers")
        .expect_err("missing credential must not create an unauthenticated request");

    assert_eq!(error.code(), ClientError::MISSING_DEVICE_CREDENTIAL);
}

#[test]
fn api_client_reports_whether_a_device_credential_is_stored() {
    let empty_store = MemoryCredentialStore::default();
    let empty_client =
        ApiClient::new("https://relay.example.test", &empty_store).expect("create client");
    assert!(!empty_client
        .has_stored_credential()
        .expect("read empty credential store"));

    let populated_store = MemoryCredentialStore::with_secret("device-secret");
    let populated_client =
        ApiClient::new("https://relay.example.test", &populated_store).expect("create client");
    assert!(populated_client
        .has_stored_credential()
        .expect("read populated credential store"));

    let empty_value_store = MemoryCredentialStore::with_secret("   ");
    let empty_value_client =
        ApiClient::new("https://relay.example.test", &empty_value_store).expect("create client");
    assert!(!empty_value_client
        .has_stored_credential()
        .expect("empty credential value is unavailable"));
}

#[test]
fn first_registration_persists_and_sends_a_client_generated_credential() {
    let (base_url, server) = one_response_server(
        "201 Created",
        r#"{"identity_id":"identity-a","created":true}"#,
        |request| {
            assert!(request.starts_with("POST /api/identities HTTP/1.1"));
            assert!(!request.contains("Authorization:"));
            assert!(request.contains("\"machine_id\":\"machine-a\""));
            assert!(request.contains("\"account_sid\":\"S-1-5-21-100\""));
            assert!(request.contains("\"credential\":\""));
        },
    );
    let store = MemoryCredentialStore::default();
    let client = ApiClient::new(base_url, &store).expect("create client");

    tauri::async_runtime::block_on(client.ensure_registered(&identity()))
        .expect("register identity");
    server.join().expect("join test relay");

    let credential = store
        .load()
        .expect("load credential")
        .expect("client credential is persisted");
    assert!(credential.len() >= 43);
}

#[test]
fn existing_identity_keeps_the_client_generated_credential_for_a_retry() {
    let (base_url, server) = one_response_server(
        "400 Bad Request",
        r#"{"error":{"code":"identity_already_registered","message":"already registered"}}"#,
        |request| assert!(request.starts_with("POST /api/identities HTTP/1.1")),
    );
    let store = MemoryCredentialStore::default();
    let client = ApiClient::new(base_url, &store).expect("create client");

    let error = tauri::async_runtime::block_on(client.ensure_registered(&identity()))
        .expect_err("existing identity must not be recovered automatically");
    server.join().expect("join test relay");

    assert_eq!(error.code(), "identity_already_registered");
    assert!(
        store
            .load()
            .expect("load credential")
            .expect("persisted credential")
            .len()
            >= 43
    );
}

#[test]
fn management_request_preserves_nested_server_error_message() {
    let (base_url, server) = one_response_server(
        "400 Bad Request",
        r#"{"error":{"code":"unsupported_protocol","message":"provider does not support messages"}}"#,
        |request| assert!(request.starts_with("POST /api/providers HTTP/1.1")),
    );
    let store = MemoryCredentialStore::with_secret("device-secret");
    let client = ApiClient::new(base_url, &store).expect("create client");

    let error = tauri::async_runtime::block_on(
        client.post::<serde_json::Value, _>("/api/providers", &serde_json::json!({})),
    )
    .expect_err("server validation failure must be returned");
    server.join().expect("join test relay");

    assert_eq!(error.code(), "unsupported_protocol");
    assert_eq!(error.message, "provider does not support messages");
}

#[test]
fn management_request_preserves_string_server_error() {
    let (base_url, server) = one_response_server(
        "400 Bad Request",
        r#"{"error":"provider does not have any models"}"#,
        |request| assert!(request.starts_with("POST /api/providers HTTP/1.1")),
    );
    let store = MemoryCredentialStore::with_secret("device-secret");
    let client = ApiClient::new(base_url, &store).expect("create client");

    let error = tauri::async_runtime::block_on(
        client.post::<serde_json::Value, _>("/api/providers", &serde_json::json!({})),
    )
    .expect_err("server validation failure must be returned");
    server.join().expect("join test relay");

    assert_eq!(error.code(), "validation_failed");
    assert_eq!(error.message, "provider does not have any models");
}

#[test]
fn management_request_hides_structured_server_error_message_on_internal_failure() {
    let (base_url, server) = one_response_server(
        "500 Internal Server Error",
        r#"{"error":{"code":"internal","message":"database error: no such table: identity_provider_configs"}}"#,
        |request| assert!(request.starts_with("POST /api/providers HTTP/1.1")),
    );
    let store = MemoryCredentialStore::with_secret("device-secret");
    let client = ApiClient::new(base_url, &store).expect("create client");

    let error = tauri::async_runtime::block_on(
        client.post::<serde_json::Value, _>("/api/providers", &serde_json::json!({})),
    )
    .expect_err("server internal failure must be returned safely");
    server.join().expect("join test relay");

    assert_eq!(error.code(), "internal");
    assert_eq!(error.message, "relay rejected the management request");
}

#[test]
fn management_request_uses_safe_fallback_for_empty_string_server_error() {
    let (base_url, server) =
        one_response_server("400 Bad Request", r#"{"error":"   "}"#, |request| {
            assert!(request.starts_with("POST /api/providers HTTP/1.1"))
        });
    let store = MemoryCredentialStore::with_secret("device-secret");
    let client = ApiClient::new(base_url, &store).expect("create client");

    let error = tauri::async_runtime::block_on(
        client.post::<serde_json::Value, _>("/api/providers", &serde_json::json!({})),
    )
    .expect_err("server validation failure must be returned");
    server.join().expect("join test relay");

    assert_eq!(error.code(), "validation_failed");
    assert_eq!(error.message, "relay rejected the management request");
}

#[test]
fn concurrent_first_registration_sends_one_request_and_shares_the_client_credential() {
    let requests = Arc::new(AtomicUsize::new(0));
    let (base_url, server) = registration_server(requests.clone());
    let store = Arc::new(MemoryCredentialStore::default());
    let gate = Arc::new(RegistrationGate::default());
    let barrier = Arc::new(Barrier::new(3));

    let workers = (0..2)
        .map(|_| {
            let base_url = base_url.clone();
            let store = store.clone();
            let gate = gate.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let client = ApiClient::new(base_url, store.as_ref()).expect("create client");
                barrier.wait();
                tauri::async_runtime::block_on(
                    client.ensure_registered_once(&identity(), gate.as_ref()),
                )
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();

    for worker in workers {
        worker
            .join()
            .expect("join registration worker")
            .expect("concurrent registration succeeds");
    }
    server.join().expect("join test relay");

    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(
        store
            .load()
            .expect("load credential")
            .expect("client credential is persisted")
            .len()
            >= 43
    );
}

fn identity() -> WindowsIdentity {
    WindowsIdentity {
        machine_id: "machine-a".into(),
        account_sid: "S-1-5-21-100".into(),
        username: "Ada".into(),
    }
}

fn one_response_server(
    status: &str,
    body: &'static str,
    assert_request: impl FnOnce(&str) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test relay");
    let address = listener.local_addr().expect("read test relay address");
    let status = status.to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept registration request");
        let request = read_http_request(&mut stream);
        assert_request(&request);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write registration response");
    });
    (format!("http://{address}"), server)
}

fn registration_server(requests: Arc<AtomicUsize>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test relay");
    listener
        .set_nonblocking(true)
        .expect("configure test relay listener");
    let address = listener.local_addr().expect("read test relay address");
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request = read_http_request(&mut stream);
                    assert!(request.starts_with("POST /api/identities HTTP/1.1"));
                    let request_number = requests.fetch_add(1, Ordering::SeqCst);
                    let (status, body) = if request_number == 0 {
                        (
                            "201 Created",
                            r#"{"identity_id":"identity-a","created":true}"#,
                        )
                    } else {
                        (
                            "400 Bad Request",
                            r#"{"error":{"code":"identity_already_registered","message":"already registered"}}"#,
                        )
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write registration response");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept registration request: {error}"),
            }
        }
    });
    (format!("http://{address}"), server)
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk).expect("read registration request");
        assert_ne!(read, 0, "registration request ended before its headers");
        request.extend_from_slice(&chunk[..read]);
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = std::str::from_utf8(&request[..header_end]).expect("request headers are UTF-8");
    let content_length = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .or_else(|| {
            headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
        })
        .expect("registration request content length")
        .parse::<usize>()
        .expect("registration request content length is numeric");
    while request.len() < header_end + content_length {
        let read = stream
            .read(&mut chunk)
            .expect("read registration request body");
        assert_ne!(read, 0, "registration request ended before its body");
        request.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(request).expect("registration request is UTF-8")
}

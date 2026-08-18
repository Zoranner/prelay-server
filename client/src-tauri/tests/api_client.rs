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
    let store = MemoryCredentialStore::with_record("device-secret", None);
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

    let populated_store = MemoryCredentialStore::with_record("device-secret", None);
    let populated_client =
        ApiClient::new("https://relay.example.test", &populated_store).expect("create client");
    assert!(populated_client
        .has_stored_credential()
        .expect("read populated credential store"));

    let empty_value_store = MemoryCredentialStore::with_record("   ", None);
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
    assert!(credential.current.len() >= 43);
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
            .current
            .len()
            >= 43
    );
}

#[test]
fn stored_credential_retries_registration_with_the_same_identity() {
    let requests = Arc::new(AtomicUsize::new(0));
    let (base_url, server) = registration_server(requests.clone());
    let store = MemoryCredentialStore::with_record("persisted-device-secret", None);
    let client = ApiClient::new(base_url, &store).expect("create client");

    tauri::async_runtime::block_on(client.ensure_registered(&identity()))
        .expect("stored credential registration retry succeeds");
    let captured_requests = server.join().expect("join test relay");

    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.load().expect("load credential"),
        Some(provider_relay_client::credential_store::CredentialRecord {
            current: "persisted-device-secret".into(),
            pending: None,
        })
    );
    let request = captured_requests
        .first()
        .expect("registration request is captured");
    assert!(request.contains("\"machine_id\":\"machine-a\""));
    assert!(request.contains("\"account_sid\":\"S-1-5-21-100\""));
    assert!(request.contains("\"credential\":\"persisted-device-secret\""));
}

#[test]
fn failed_registration_retries_with_the_persisted_credential() {
    let (base_url, server) = retry_registration_server();
    let store = MemoryCredentialStore::default();
    let gate = RegistrationGate::default();
    let client = ApiClient::new(base_url, &store).expect("create client");

    tauri::async_runtime::block_on(client.ensure_registered_once(&identity(), &gate))
        .expect_err("dropped registration response must fail");

    tauri::async_runtime::block_on(client.ensure_registered_once(&identity(), &gate))
        .expect("persisted credential registration retry succeeds");
    let requests = server.join().expect("join test relay");
    let credential = store
        .load()
        .expect("load credential")
        .expect("client credential is persisted");

    assert_eq!(requests.len(), 2);
    for request in requests {
        assert!(request.contains("\"machine_id\":\"machine-a\""));
        assert!(request.contains("\"account_sid\":\"S-1-5-21-100\""));
        assert!(request.contains(&format!("\"credential\":\"{}\"", credential.current)));
    }
}

#[test]
fn registration_gate_serializes_calls_without_caching_registration_state() {
    let (base_url, server) = two_registration_response_server();
    let store = MemoryCredentialStore::with_record("persisted-device-secret", None);
    let gate = RegistrationGate::default();
    let client = ApiClient::new(base_url, &store).expect("create client");

    tauri::async_runtime::block_on(client.ensure_registered_once(&identity(), &gate))
        .expect("first registration confirmation succeeds");
    tauri::async_runtime::block_on(client.ensure_registered_once(&identity(), &gate))
        .expect("second registration confirmation succeeds");

    assert_eq!(server.join().expect("join test relay"), 2);
}

#[test]
fn pending_credential_registration_confirms_a_rotation_whose_response_was_lost() {
    let (base_url, server) = two_response_server(
        [
            ("200 OK", r#"{"identity_id":"identity-a","created":false}"#),
            ("200 OK", r#"{}"#),
        ],
        |requests| {
            assert!(requests[0].starts_with("POST /api/identities HTTP/1.1"));
            assert!(requests[0].contains("\"credential\":\"credential-new\""));
            assert!(requests[1].contains("Authorization: Bearer credential-new"));
        },
    );
    let store = MemoryCredentialStore::with_record("credential-old", Some("credential-new"));
    let client = ApiClient::new(base_url, &store).expect("create client");

    tauri::async_runtime::block_on(client.ensure_registered(&identity()))
        .expect("pending credential confirms existing identity");
    tauri::async_runtime::block_on(client.get::<serde_json::Value>("/api/providers"))
        .expect("confirmed credential authenticates requests");
    server.join().expect("join test relay");

    assert_eq!(
        store.load().expect("load credential"),
        Some(provider_relay_client::credential_store::CredentialRecord {
            current: "credential-new".into(),
            pending: None,
        })
    );
}

#[test]
fn rejected_pending_registration_falls_back_to_current_credential() {
    let (base_url, server) = two_response_server(
        [
            (
                "400 Bad Request",
                r#"{"error":{"code":"identity_already_registered","message":"already registered"}}"#,
            ),
            ("200 OK", r#"{"identity_id":"identity-a","created":false}"#),
        ],
        |requests| {
            assert!(requests[0].contains("\"credential\":\"credential-new\""));
            assert!(requests[1].contains("\"credential\":\"credential-old\""));
        },
    );
    let store = MemoryCredentialStore::with_record("credential-old", Some("credential-new"));
    let client = ApiClient::new(base_url, &store).expect("create client");

    tauri::async_runtime::block_on(client.ensure_registered(&identity()))
        .expect("current credential confirms the unrotated identity");
    server.join().expect("join test relay");

    assert_eq!(
        store.load().expect("load credential"),
        Some(provider_relay_client::credential_store::CredentialRecord {
            current: "credential-old".into(),
            pending: None,
        })
    );
}

#[test]
fn accepted_pending_credential_becomes_current_after_an_authenticated_request() {
    let (base_url, server) = one_response_server("200 OK", r#"{}"#, |request| {
        assert!(request.contains("Authorization: Bearer credential-new"));
    });
    let store = MemoryCredentialStore::with_record("credential-old", Some("credential-new"));
    let client = ApiClient::new(base_url, &store).expect("create client");

    tauri::async_runtime::block_on(client.get::<serde_json::Value>("/api/providers"))
        .expect("pending credential is accepted");
    server.join().expect("join test relay");

    assert_eq!(
        store.load().expect("load credential"),
        Some(provider_relay_client::credential_store::CredentialRecord {
            current: "credential-new".into(),
            pending: None,
        })
    );
}

#[test]
fn rejected_pending_credential_falls_back_to_current_and_is_discarded() {
    let (base_url, server) = two_response_server(
        [
            ("401 Unauthorized", r#"{"error":"invalid credential"}"#),
            ("200 OK", r#"{}"#),
        ],
        |requests| {
            assert!(requests[0].contains("Authorization: Bearer credential-new"));
            assert!(requests[1].contains("Authorization: Bearer credential-old"));
        },
    );
    let store = MemoryCredentialStore::with_record("credential-old", Some("credential-new"));
    let client = ApiClient::new(base_url, &store).expect("create client");

    tauri::async_runtime::block_on(client.get::<serde_json::Value>("/api/providers"))
        .expect("current credential recovers the request");
    server.join().expect("join test relay");

    assert_eq!(
        store.load().expect("load credential"),
        Some(provider_relay_client::credential_store::CredentialRecord {
            current: "credential-old".into(),
            pending: None,
        })
    );
}

#[test]
fn server_failure_preserves_pending_credential_for_later_recovery() {
    let (base_url, server) = one_response_server(
        "500 Internal Server Error",
        r#"{"error":{"code":"internal","message":"ignored"}}"#,
        |request| assert!(request.contains("Authorization: Bearer credential-new")),
    );
    let store = MemoryCredentialStore::with_record("credential-old", Some("credential-new"));
    let client = ApiClient::new(base_url, &store).expect("create client");

    let error = tauri::async_runtime::block_on(client.get::<serde_json::Value>("/api/providers"))
        .expect_err("server failure must fail the request");
    server.join().expect("join test relay");

    assert_eq!(error.code(), "internal");
    assert_eq!(
        store.load().expect("load credential"),
        Some(provider_relay_client::credential_store::CredentialRecord {
            current: "credential-old".into(),
            pending: Some("credential-new".into()),
        })
    );
}

#[test]
fn management_request_preserves_nested_server_error_message() {
    let (base_url, server) = one_response_server(
        "400 Bad Request",
        r#"{"error":{"code":"unsupported_protocol","message":"provider does not support messages"}}"#,
        |request| assert!(request.starts_with("POST /api/providers HTTP/1.1")),
    );
    let store = MemoryCredentialStore::with_record("device-secret", None);
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
    let store = MemoryCredentialStore::with_record("device-secret", None);
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
    let store = MemoryCredentialStore::with_record("device-secret", None);
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
    let store = MemoryCredentialStore::with_record("device-secret", None);
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
fn concurrent_registration_confirmation_is_serialized_without_being_cached() {
    let (base_url, server) = two_registration_response_server();
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
    assert_eq!(server.join().expect("join test relay"), 2);
    assert!(
        store
            .load()
            .expect("load credential")
            .expect("client credential is persisted")
            .current
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

fn registration_server(requests: Arc<AtomicUsize>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test relay");
    listener
        .set_nonblocking(true)
        .expect("configure test relay listener");
    let address = listener.local_addr().expect("read test relay address");
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(300);
        let mut captured_requests = Vec::new();
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("configure accepted relay connection");
                    let request = read_http_request(&mut stream);
                    assert!(request.starts_with("POST /api/identities HTTP/1.1"));
                    captured_requests.push(request);
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
        captured_requests
    });
    (format!("http://{address}"), server)
}

fn two_registration_response_server() -> (String, thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test relay");
    let address = listener.local_addr().expect("read test relay address");
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept registration request");
            let request = read_http_request(&mut stream);
            assert!(request.starts_with("POST /api/identities HTTP/1.1"));
            let body = r#"{"identity_id":"identity-a","created":false}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write registration response");
        }
        2
    });
    (format!("http://{address}"), server)
}

fn two_response_server(
    responses: [(&'static str, &'static str); 2],
    assert_requests: impl FnOnce(&[String]) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test relay");
    let address = listener.local_addr().expect("read test relay address");
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().expect("accept relay request");
            requests.push(read_http_request(&mut stream));
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write relay response");
        }
        assert_requests(&requests);
    });
    (format!("http://{address}"), server)
}

fn retry_registration_server() -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test relay");
    let address = listener.local_addr().expect("read test relay address");
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept registration request");
            requests.push(read_http_request(&mut stream));
            if attempt == 1 {
                let body = r#"{"identity_id":"identity-a","created":true}"#;
                let response = format!(
                    "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write registration response");
            }
        }
        requests
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
        .map(|value| {
            value
                .parse::<usize>()
                .expect("registration request content length is numeric")
        })
        .unwrap_or_default();
    while request.len() < header_end + content_length {
        let read = stream
            .read(&mut chunk)
            .expect("read registration request body");
        assert_ne!(read, 0, "registration request ended before its body");
        request.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(request).expect("registration request is UTF-8")
}

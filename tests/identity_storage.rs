mod support;

use std::{
    ffi::OsString,
    sync::{Mutex, OnceLock},
};

use prelay_protocol::{
    CreateEndpointRequest, CreateProviderRequest, EndpointModelInput, ProtocolErrorCode,
};
use prelay_server::{
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalResponse, InternalRole,
    },
    identity::credential::generate_credential,
    stats::RequestLogInsert,
    storage::{MasterKey, ProtocolAccess, ResponseSessionInsert, Storage, StorageError},
};

const TEST_MASTER_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

fn provider_input(name: &str, api_key: &str) -> CreateProviderRequest {
    CreateProviderRequest {
        name: name.to_string(),
        provider_type: "openai_compatible".to_string(),
        base_url: "https://provider.example".to_string(),
        api_key: api_key.to_string(),
        capabilities: None,
        models: vec!["test-model".to_string()],
    }
}

fn request_log(provider_id: &str, upstream_latency_ms: i64) -> RequestLogInsert {
    RequestLogInsert {
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: "chat_completions".to_string(),
        endpoint_name: "Test endpoint".to_string(),
        provider_id: provider_id.to_string(),
        provider_name: "Test provider".to_string(),
        model_requested: "shared-model".to_string(),
        model_upstream: "test-model".to_string(),
        status: "success".to_string(),
        http_status: 200,
        latency_ms: upstream_latency_ms,
        upstream_latency_ms: Some(upstream_latency_ms),
        ..Default::default()
    }
}

#[tokio::test]
async fn identity_credentials_are_hashed_and_provider_keys_are_encrypted() {
    let storage = support::test_storage().await;
    let credential = generate_credential();
    let registered = storage
        .register_identity("machine-a", "S-1-5-21-100", &credential)
        .await
        .expect("register identity");

    assert!(storage
        .authenticate_identity(&credential)
        .await
        .expect("authenticate credential")
        .is_some());
    assert!(storage
        .authenticate_identity("wrong-device-credential")
        .await
        .expect("reject wrong credential")
        .is_none());
    assert_ne!(
        storage
            .identity_credential_hash(&registered.identity_id)
            .await
            .expect("load credential hash"),
        credential
    );

    let provider_id = storage
        .create_provider(
            &registered.identity_id,
            provider_input("Test provider", "test-provider-key"),
        )
        .await
        .expect("create provider");
    assert_ne!(
        storage
            .raw_provider_key_ciphertext(&registered.identity_id, &provider_id)
            .await
            .expect("load provider ciphertext"),
        "test-provider-key"
    );
    assert_eq!(
        storage
            .decrypt_provider_key(&registered.identity_id, &provider_id)
            .await
            .expect("decrypt provider key"),
        "test-provider-key"
    );
}

#[tokio::test]
async fn stable_identity_key_retries_only_with_the_same_credential() {
    let storage = support::test_storage().await;
    let credential_a = generate_credential();
    let credential_b = generate_credential();

    let created = storage
        .register_identity("machine-a", "S-1-5-21-100", &credential_a)
        .await
        .expect("register identity");
    let retried = storage
        .register_identity("machine-a", "S-1-5-21-100", &credential_a)
        .await
        .expect("retry matching registration");

    assert!(created.created);
    assert!(!retried.created);
    assert_eq!(created.identity_id, retried.identity_id);
    let error = storage
        .register_identity("machine-a", "S-1-5-21-100", &credential_b)
        .await
        .expect_err("reject duplicate stable identity key");
    assert!(matches!(error, StorageError::IdentityAlreadyRegistered));
    assert_eq!(error.code(), ProtocolErrorCode::IdentityAlreadyRegistered);
}

#[tokio::test]
async fn provider_and_endpoint_writes_leave_no_partial_resources_after_validation_errors() {
    let storage = support::test_storage().await;
    let identity = storage
        .register_identity("machine-a", "S-1-5-21-100", &generate_credential())
        .await
        .expect("register identity");

    let mut invalid_provider = provider_input("Invalid provider", "test-provider-key");
    invalid_provider.models = vec!["test-model".to_string(), " test-model ".to_string()];
    let error = storage
        .create_provider(&identity.identity_id, invalid_provider)
        .await
        .expect_err("reject duplicate provider model");
    assert!(matches!(error, StorageError::ValidationFailed(_)));
    assert!(storage
        .list_providers(&identity.identity_id)
        .await
        .expect("list providers")
        .is_empty());

    let provider_id = storage
        .create_provider(
            &identity.identity_id,
            provider_input("Valid provider", "test-provider-key"),
        )
        .await
        .expect("create provider");
    let duplicate = EndpointModelInput {
        provider_id: provider_id.clone(),
        upstream_model: "test-model".to_string(),
        model_name: Some("public-model".to_string()),
    };
    let error = storage
        .create_interface(
            &identity.identity_id,
            CreateEndpointRequest {
                name: "Invalid endpoint".to_string(),
                protocol: None,
                models: vec![
                    duplicate.clone(),
                    EndpointModelInput {
                        upstream_model: " test-model ".to_string(),
                        model_name: Some(" public-model ".to_string()),
                        ..duplicate
                    },
                ],
            },
        )
        .await
        .expect_err("reject duplicate endpoint mapping");
    assert!(matches!(error, StorageError::ValidationFailed(_)));
    assert!(storage
        .list_endpoints(&identity.identity_id)
        .await
        .expect("list endpoints")
        .is_empty());
}

#[tokio::test]
async fn model_candidates_keep_mapping_order_then_prefer_observed_latency() {
    let storage = support::test_storage().await;
    let identity = storage
        .register_identity(
            "machine-priority",
            "S-1-5-21-priority",
            &generate_credential(),
        )
        .await
        .expect("register identity");
    let primary_provider_id = storage
        .create_provider(
            &identity.identity_id,
            provider_input("Primary provider", "test-primary-key"),
        )
        .await
        .expect("create primary provider");
    let backup_provider_id = storage
        .create_provider(
            &identity.identity_id,
            provider_input("Backup provider", "test-backup-key"),
        )
        .await
        .expect("create backup provider");
    let endpoint = storage
        .create_interface(
            &identity.identity_id,
            CreateEndpointRequest {
                name: "Priority endpoint".to_string(),
                protocol: Some("all".to_string()),
                models: vec![
                    EndpointModelInput {
                        provider_id: primary_provider_id.clone(),
                        upstream_model: "test-model".to_string(),
                        model_name: Some("shared-model".to_string()),
                    },
                    EndpointModelInput {
                        provider_id: backup_provider_id.clone(),
                        upstream_model: "test-model".to_string(),
                        model_name: Some("shared-model".to_string()),
                    },
                ],
            },
        )
        .await
        .expect("create endpoint with primary and backup");
    let access = ProtocolAccess {
        identity_id: identity.identity_id,
        endpoint_id: endpoint.id,
        endpoint_name: endpoint.name,
    };

    let candidates = storage
        .resolve_protocol_models(&access, "shared-model")
        .await
        .expect("resolve candidates");
    assert_eq!(candidates[0].provider.id, primary_provider_id);
    assert_eq!(candidates[1].provider.id, backup_provider_id);

    storage
        .insert_request_log_with_id(
            &access.identity_id,
            "primary-latency".to_string(),
            request_log(&primary_provider_id, 120),
        )
        .await
        .expect("record primary latency");
    storage
        .insert_request_log_with_id(
            &access.identity_id,
            "backup-latency".to_string(),
            request_log(&backup_provider_id, 20),
        )
        .await
        .expect("record backup latency");

    let selected = storage
        .select_protocol_model_candidates(&access, "shared-model")
        .await
        .expect("select candidates by latency");
    assert_eq!(selected[0].provider.id, backup_provider_id);

    storage
        .remember_protocol_model_provider(&access, "shared-model", &primary_provider_id)
        .await
        .expect("remember current provider");
    let selected = storage
        .select_protocol_model_candidates(&access, "shared-model")
        .await
        .expect("select candidates with remembered provider");
    assert_eq!(selected[0].provider.id, primary_provider_id);
}

#[tokio::test]
async fn provider_keys_and_response_sessions_stay_scoped_to_the_identity() {
    let storage = support::test_storage().await;
    let (identity_a, provider_a) = seed_identity_and_provider(&storage, "a").await;
    let (identity_b, provider_b) = seed_identity_and_provider(&storage, "b").await;

    let error = storage
        .decrypt_provider_key(&identity_a, &provider_b)
        .await
        .expect_err("identity A cannot read identity B provider key");
    assert!(matches!(error, StorageError::ProviderNotFound));

    let input_a = vec![message("identity-a input")];
    let input_b = vec![message("identity-b input")];
    storage
        .save_response_session(ResponseSessionInsert {
            identity_id: &identity_a,
            response_id: "shared-response",
            previous_response_id: None,
            provider_id: &provider_a,
            model: "test-model",
            input_messages: &input_a,
            response: &response("response-a", "identity-a output"),
        })
        .await
        .expect("save identity A session");
    storage
        .save_response_session(ResponseSessionInsert {
            identity_id: &identity_b,
            response_id: "shared-response",
            previous_response_id: None,
            provider_id: &provider_b,
            model: "test-model",
            input_messages: &input_b,
            response: &response("response-b", "identity-b output"),
        })
        .await
        .expect("save identity B session");

    assert_eq!(
        storage
            .load_response_session_messages(&identity_a, "shared-response")
            .await
            .expect("load identity A session"),
        Some(vec![
            message("identity-a input"),
            assistant_message("identity-a output"),
        ])
    );
    assert_eq!(
        storage
            .load_response_session_messages(&identity_b, "shared-response")
            .await
            .expect("load identity B session"),
        Some(vec![
            message("identity-b input"),
            assistant_message("identity-b output"),
        ])
    );
}

#[tokio::test]
async fn credential_rotation_rejects_a_stale_authenticated_credential_hash() {
    let storage = support::test_storage().await;
    let current_credential = generate_credential();
    let new_credential = generate_credential();
    let registered = storage
        .register_identity("machine-a", "S-1-5-21-100", &current_credential)
        .await
        .expect("register identity");
    let credential_hash = storage
        .identity_credential_hash(&registered.identity_id)
        .await
        .expect("load authenticated credential hash");

    assert!(
        storage
            .rotate_identity_credential(&registered.identity_id, &credential_hash, &new_credential)
            .await
            .expect("rotate credential")
            .rotated
    );
    let error = storage
        .rotate_identity_credential(
            &registered.identity_id,
            &credential_hash,
            &generate_credential(),
        )
        .await
        .expect_err("reject stale authenticated credential hash");
    assert_eq!(error.code(), ProtocolErrorCode::InvalidCredential);
    assert!(storage
        .authenticate_identity(&current_credential)
        .await
        .expect("authenticate old credential")
        .is_none());
    assert!(storage
        .authenticate_identity(&new_credential)
        .await
        .expect("authenticate rotated credential")
        .is_some());
}

#[tokio::test]
async fn provider_write_requires_an_existing_identity() {
    let storage = support::test_storage().await;
    let error = storage
        .create_provider(
            "missing-identity",
            provider_input("Test provider", "test-provider-key"),
        )
        .await
        .expect_err("reject provider without an owner");
    assert!(matches!(error, StorageError::IdentityNotFound));
    assert_eq!(error.code(), ProtocolErrorCode::NotFound);
}

#[test]
fn master_key_requires_base64_encoded_32_bytes() {
    assert!(MasterKey::from_base64(TEST_MASTER_KEY).is_ok());
    assert!(MasterKey::from_base64("not base64").is_err());
    assert!(MasterKey::from_base64("AAAA").is_err());
}

#[test]
fn master_key_environment_requires_a_valid_base64_encoded_32_byte_value() {
    let _lock = master_key_environment_lock()
        .lock()
        .expect("lock master key environment");
    let _restore = MasterKeyEnvironmentRestore::capture();

    std::env::remove_var("ENCRYPTION_KEY");
    assert!(MasterKey::from_environment().is_err());
    std::env::set_var("ENCRYPTION_KEY", "not base64");
    assert!(MasterKey::from_environment().is_err());
    std::env::set_var("ENCRYPTION_KEY", "AAAA");
    assert!(MasterKey::from_environment().is_err());
}

async fn seed_identity_and_provider(storage: &Storage, suffix: &str) -> (String, String) {
    let identity = storage
        .register_identity(
            &format!("machine-{suffix}"),
            &format!("sid-{suffix}"),
            &generate_credential(),
        )
        .await
        .expect("register identity");
    let provider_id = storage
        .create_provider(
            &identity.identity_id,
            provider_input(&format!("Provider {suffix}"), &format!("test-{suffix}-key")),
        )
        .await
        .expect("create provider");
    (identity.identity_id, provider_id)
}

fn message(text: &str) -> InternalMessage {
    InternalMessage {
        role: InternalRole::User,
        content: vec![InternalContentPart::Text(text.to_string())],
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    }
}

fn assistant_message(text: &str) -> InternalMessage {
    InternalMessage {
        role: InternalRole::Assistant,
        content: vec![InternalContentPart::Text(text.to_string())],
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    }
}

fn response(id: &str, text: &str) -> InternalResponse {
    InternalResponse {
        id: id.to_string(),
        model: "test-model".to_string(),
        output: vec![InternalOutputItem::Message {
            id: format!("{id}-message"),
            role: InternalRole::Assistant,
            content: vec![InternalContentPart::Text(text.to_string())],
        }],
        usage: None,
    }
}

fn master_key_environment_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct MasterKeyEnvironmentRestore {
    original: Option<OsString>,
}

impl MasterKeyEnvironmentRestore {
    fn capture() -> Self {
        Self {
            original: std::env::var_os("ENCRYPTION_KEY"),
        }
    }
}

impl Drop for MasterKeyEnvironmentRestore {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var("ENCRYPTION_KEY", value),
            None => std::env::remove_var("ENCRYPTION_KEY"),
        }
    }
}

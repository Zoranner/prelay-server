use prelay_protocol::{
    CreateEndpointRequest, CreateProviderRequest, EndpointModelInput, ProtocolErrorCode,
};
use prelay_server::{
    bridge::internal::{InternalContentPart, InternalMessage, InternalOutputItem, InternalRole},
    identity::credential::generate_credential,
    storage::{sessions::load_response_session_messages, MasterKey, Storage, StorageError},
};
use sqlx::sqlite::SqlitePoolOptions;
use std::{
    ffi::OsString,
    sync::{Mutex, OnceLock},
};

const TEST_MASTER_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

async fn test_storage() -> Storage {
    Storage::in_memory_from_base64(TEST_MASTER_KEY)
        .await
        .expect("create in-memory storage")
}

fn provider_input(api_key: &str) -> CreateProviderRequest {
    CreateProviderRequest {
        name: "Test provider".to_string(),
        provider_type: "openai_compatible".to_string(),
        base_url: "https://provider.example".to_string(),
        api_key: api_key.to_string(),
        capabilities: None,
        models: vec!["test-model".to_string()],
    }
}

#[tokio::test]
async fn identity_credentials_are_hashed_and_provider_keys_are_encrypted() {
    let storage = test_storage().await;
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
        .create_provider(&registered.identity_id, provider_input("sk-secret"))
        .await
        .expect("create provider");
    assert_ne!(
        storage
            .raw_provider_key_ciphertext(&registered.identity_id, &provider_id)
            .await
            .expect("load provider ciphertext"),
        "sk-secret"
    );
    assert_eq!(
        storage
            .decrypt_provider_key(&registered.identity_id, &provider_id)
            .await
            .expect("decrypt provider key"),
        "sk-secret"
    );

    let second_provider_id = storage
        .create_provider(&registered.identity_id, provider_input("sk-secret"))
        .await
        .expect("create second provider");
    assert_ne!(
        storage
            .raw_provider_key_ciphertext(&registered.identity_id, &provider_id)
            .await
            .expect("load first provider ciphertext"),
        storage
            .raw_provider_key_ciphertext(&registered.identity_id, &second_provider_id)
            .await
            .expect("load second provider ciphertext")
    );
}

#[tokio::test]
async fn authentication_updates_identity_display_name() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create in-memory pool");
    let storage = Storage::initialize(pool.clone(), MasterKey::from_bytes([0; 32]))
        .await
        .expect("initialize storage");
    let credential = generate_credential();
    let registered = storage
        .register_identity_with_display_name(
            "machine-a",
            "S-1-5-21-100",
            &credential,
            Some("Initial name"),
        )
        .await
        .expect("register identity");

    storage
        .authenticate_identity_with_display_name(&credential, Some("Updated name"))
        .await
        .expect("authenticate credential");

    let display_name =
        sqlx::query_scalar::<_, String>("SELECT display_name FROM identities WHERE id = ?")
            .bind(registered.identity_id)
            .fetch_one(&pool)
            .await
            .expect("load display name");
    assert_eq!(display_name, "Updated name");
}

#[tokio::test]
async fn endpoint_model_candidates_allow_same_alias_and_keep_mapping_order() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create sqlite pool");
    let storage = Storage::initialize(pool.clone(), MasterKey::from_bytes([0; 32]))
        .await
        .expect("create in-memory storage");
    let identity = storage
        .register_identity(
            "machine-priority",
            "S-1-5-21-priority",
            &generate_credential(),
        )
        .await
        .expect("register identity");
    let primary_provider_id = storage
        .create_provider(&identity.identity_id, provider_input("sk-primary"))
        .await
        .expect("create primary provider");
    let backup_provider_id = storage
        .create_provider(&identity.identity_id, provider_input("sk-backup"))
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

    let candidates = storage
        .resolve_protocol_models(
            &prelay_server::storage::ProtocolAccess {
                identity_id: identity.identity_id.clone(),
                endpoint_id: endpoint.id.clone(),
                endpoint_name: endpoint.name.clone(),
            },
            "shared-model",
        )
        .await
        .expect("resolve candidates");

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].provider.id, primary_provider_id);
    assert_eq!(candidates[1].provider.id, backup_provider_id);

    for (id, provider_id, upstream_latency_ms) in [
        ("primary-latency", primary_provider_id.as_str(), 120_i64),
        ("backup-latency", backup_provider_id.as_str(), 20_i64),
    ] {
        sqlx::query(
            "INSERT INTO identity_request_logs (\
                id, identity_id, created_at, provider_id, status, upstream_latency_ms\
             ) VALUES (?, ?, ?, ?, 'success', ?)",
        )
        .bind(id)
        .bind(&identity.identity_id)
        .bind("2026-08-23T00:00:00Z")
        .bind(provider_id)
        .bind(upstream_latency_ms)
        .execute(&pool)
        .await
        .expect("insert successful request latency");
    }

    let access = prelay_server::storage::ProtocolAccess {
        identity_id: identity.identity_id,
        endpoint_id: endpoint.id,
        endpoint_name: endpoint.name,
    };
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
        .expect("select candidates with current provider");
    assert_eq!(selected[0].provider.id, primary_provider_id);
}

#[tokio::test]
async fn registration_retries_only_when_the_client_credential_matches() {
    let storage = test_storage().await;
    let credential_a = generate_credential();
    let credential_b = generate_credential();

    let created = storage
        .register_identity("machine-a", "S-1-5-21-100", &credential_a)
        .await
        .unwrap();
    let retried = storage
        .register_identity("machine-a", "S-1-5-21-100", &credential_a)
        .await
        .unwrap();

    assert!(created.created);
    assert!(!retried.created);
    assert_eq!(created.identity_id, retried.identity_id);
    assert_eq!(
        storage
            .register_identity("machine-a", "S-1-5-21-100", &credential_b)
            .await
            .unwrap_err()
            .code(),
        ProtocolErrorCode::IdentityAlreadyRegistered,
    );
}

#[tokio::test]
async fn credential_rotation_rejects_a_stale_authenticated_credential_hash() {
    let storage = test_storage().await;
    let current_credential = generate_credential();
    let new_credential = generate_credential();
    let stale_credential = generate_credential();
    let registered = storage
        .register_identity("machine-a", "S-1-5-21-100", &current_credential)
        .await
        .expect("register identity");
    let credential_hash = storage
        .identity_credential_hash(&registered.identity_id)
        .await
        .expect("load authenticated credential hash");

    let rotated = storage
        .rotate_identity_credential(&registered.identity_id, &credential_hash, &new_credential)
        .await
        .expect("rotate with the authenticated credential hash");
    let stale_rotation = storage
        .rotate_identity_credential(&registered.identity_id, &credential_hash, &stale_credential)
        .await
        .expect_err("reject stale authenticated credential hash");

    assert_eq!(stale_rotation.code(), ProtocolErrorCode::InvalidCredential);
    assert!(rotated.rotated);
    assert!(storage
        .authenticate_identity(&current_credential)
        .await
        .expect("reject old credential")
        .is_none());
    assert!(storage
        .authenticate_identity(&new_credential)
        .await
        .expect("authenticate rotated credential")
        .is_some());
}

#[tokio::test]
async fn foreign_keys_are_enforced_after_multiple_pool_checkouts() {
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .min_connections(2)
        .connect("sqlite:file:identity-storage-foreign-keys?mode=memory&cache=shared")
        .await
        .expect("create sqlite pool");
    let _storage = Storage::initialize(pool.clone(), MasterKey::from_bytes([0; 32]))
        .await
        .expect("initialize identity storage");

    let first_connection = pool.acquire().await.expect("checkout first connection");
    let second_connection = pool.acquire().await.expect("checkout second connection");
    drop(first_connection);
    drop(second_connection);

    let error = sqlx::query(
        "INSERT INTO identity_provider_models (id, provider_id, model_name, created_at) \\
         VALUES (?, ?, ?, ?)",
    )
    .bind("orphan-model")
    .bind("missing-provider")
    .bind("model-a")
    .bind("2026-08-13T00:00:00Z")
    .execute(&pool)
    .await
    .expect_err("reject orphan provider model from pooled connection");
    assert!(matches!(error, sqlx::Error::Database(_)));
}

#[tokio::test]
async fn provider_key_reads_are_scoped_to_the_identity() {
    let storage = test_storage().await;
    let credential_a = generate_credential();
    let credential_b = generate_credential();
    let identity_a = storage
        .register_identity("machine-a", "S-1-5-21-100", &credential_a)
        .await
        .expect("register identity A");
    let identity_b = storage
        .register_identity("machine-b", "S-1-5-21-200", &credential_b)
        .await
        .expect("register identity B");
    let provider_a = storage
        .create_provider(&identity_a.identity_id, provider_input("sk-a"))
        .await
        .expect("create provider A");
    let provider_b = storage
        .create_provider(&identity_b.identity_id, provider_input("sk-b"))
        .await
        .expect("create provider B");

    assert_ne!(
        storage
            .raw_provider_key_ciphertext(&identity_a.identity_id, &provider_a)
            .await
            .expect("read A ciphertext"),
        "sk-a"
    );
    assert_eq!(
        storage
            .decrypt_provider_key(&identity_a.identity_id, &provider_a)
            .await
            .expect("decrypt A key"),
        "sk-a"
    );

    for result in [
        storage
            .raw_provider_key_ciphertext(&identity_a.identity_id, &provider_b)
            .await,
        storage
            .decrypt_provider_key(&identity_a.identity_id, &provider_b)
            .await,
    ] {
        let error = result.expect_err("identity A cannot read identity B provider key");
        assert!(matches!(error, StorageError::ProviderNotFound));
        assert_eq!(error.code(), ProtocolErrorCode::NotFound);
    }
}

#[tokio::test]
async fn stable_key_rejects_a_different_client_credential() {
    let storage = test_storage().await;
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
    assert!(!retried.created);
    assert_eq!(retried.identity_id, created.identity_id);

    let error = storage
        .register_identity("machine-a", "S-1-5-21-100", &credential_b)
        .await
        .expect_err("reject duplicate stable identity key");
    assert_eq!(error.code(), ProtocolErrorCode::IdentityAlreadyRegistered);
    assert!(matches!(error, StorageError::IdentityAlreadyRegistered));
}

#[tokio::test]
async fn provider_write_requires_an_existing_identity() {
    let storage = test_storage().await;

    let error = storage
        .create_provider("missing-identity", provider_input("sk-secret"))
        .await
        .expect_err("reject provider without an owner");
    assert_eq!(error.code(), ProtocolErrorCode::NotFound);
    assert!(matches!(error, StorageError::IdentityNotFound));
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

    std::env::remove_var("PRELAY_MASTER_KEY");
    assert!(MasterKey::from_environment().is_err());

    std::env::set_var("PRELAY_MASTER_KEY", "not base64");
    assert!(MasterKey::from_environment().is_err());

    std::env::set_var("PRELAY_MASTER_KEY", "AAAA");
    assert!(MasterKey::from_environment().is_err());
}

#[tokio::test]
async fn response_session_schema_upgrade_preserves_rows_and_scopes_response_ids() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create sqlite pool");
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("enable foreign keys");
    sqlx::query(
        "CREATE TABLE identities (\
            id TEXT PRIMARY KEY,\
            machine_id TEXT NOT NULL,\
            account_sid TEXT NOT NULL,\
            credential_hash TEXT NOT NULL,\
            created_at TEXT NOT NULL,\
            last_active_at TEXT NOT NULL,\
            UNIQUE(machine_id, account_sid)\
        )",
    )
    .execute(&pool)
    .await
    .expect("create identities table");
    sqlx::query(
        "CREATE TABLE identity_provider_configs (\
            id TEXT PRIMARY KEY,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            name TEXT NOT NULL,\
            provider_type TEXT NOT NULL,\
            base_url TEXT NOT NULL,\
            api_key_ciphertext TEXT NOT NULL,\
            capabilities_json TEXT,\
            created_at TEXT NOT NULL\
        )",
    )
    .execute(&pool)
    .await
    .expect("create provider table");
    sqlx::query(
        "CREATE TABLE identity_response_sessions (\
            response_id TEXT PRIMARY KEY,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            previous_response_id TEXT,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            model TEXT NOT NULL,\
            input_messages_json TEXT NOT NULL,\
            output_items_json TEXT NOT NULL,\
            created_at TEXT NOT NULL\
        )",
    )
    .execute(&pool)
    .await
    .expect("create old response sessions table");
    let identity_a_messages = vec![InternalMessage {
        role: InternalRole::User,
        content: vec![InternalContentPart::Text("identity-a input".to_string())],
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    }];
    let identity_b_messages = vec![InternalMessage {
        role: InternalRole::User,
        content: vec![InternalContentPart::Text("identity-b input".to_string())],
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    }];

    for identity_id in ["identity-a", "identity-b"] {
        sqlx::query(
            "INSERT INTO identities (id, machine_id, account_sid, credential_hash, created_at, last_active_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(identity_id)
        .bind(format!("machine-{identity_id}"))
        .bind(format!("sid-{identity_id}"))
        .bind("credential-hash")
        .bind("2026-08-13T00:00:00Z")
        .bind("2026-08-13T00:00:00Z")
        .execute(&pool)
        .await
        .expect("insert identity");
        sqlx::query(
            "INSERT INTO identity_provider_configs (id, identity_id, name, provider_type, base_url, \
             api_key_ciphertext, capabilities_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("provider-{identity_id}"))
        .bind(identity_id)
        .bind("Provider")
        .bind("openai_compatible")
        .bind("https://provider.example")
        .bind("ciphertext")
        .bind(Option::<String>::None)
        .bind("2026-08-13T00:00:00Z")
        .execute(&pool)
        .await
        .expect("insert provider");
    }
    sqlx::query(
        "INSERT INTO identity_response_sessions (response_id, identity_id, previous_response_id, provider_id, \
         model, input_messages_json, output_items_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("resp-shared")
    .bind("identity-a")
    .bind(Option::<String>::None)
    .bind("provider-identity-a")
    .bind("model-a")
    .bind(serde_json::to_string(&identity_a_messages).expect("serialize identity A messages"))
    .bind(serde_json::to_string(&Vec::<InternalOutputItem>::new()).expect("serialize output"))
    .bind("2026-08-13T00:00:00Z")
    .execute(&pool)
    .await
    .expect("insert legacy response session");

    let _storage = Storage::initialize(pool.clone(), MasterKey::from_bytes([0; 32]))
        .await
        .expect("upgrade response session schema");

    let preserved = sqlx::query_as::<_, (String, String)>(
        "SELECT identity_id, provider_id \
         FROM identity_response_sessions WHERE response_id = ? AND identity_id = ?",
    )
    .bind("resp-shared")
    .bind("identity-a")
    .fetch_one(&pool)
    .await
    .expect("preserve legacy response session");
    assert_eq!(
        preserved,
        ("identity-a".to_string(), "provider-identity-a".to_string(),)
    );

    sqlx::query(
        "INSERT INTO identity_response_sessions (response_id, identity_id, previous_response_id, provider_id, \
         model, input_messages_json, output_items_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("resp-shared")
    .bind("identity-b")
    .bind(Option::<String>::None)
    .bind("provider-identity-b")
    .bind("model-b")
    .bind(serde_json::to_string(&identity_b_messages).expect("serialize identity B messages"))
    .bind(serde_json::to_string(&Vec::<InternalOutputItem>::new()).expect("serialize output"))
    .bind("2026-08-13T00:00:01Z")
    .execute(&pool)
    .await
    .expect("allow the same response id for another identity");

    let sessions = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM identity_response_sessions WHERE response_id = ?",
    )
    .bind("resp-shared")
    .fetch_one(&pool)
    .await
    .expect("count response sessions");
    assert_eq!(sessions, 2);
    assert_eq!(
        load_response_session_messages(&pool, "identity-a", "resp-shared")
            .await
            .expect("load identity A response session"),
        Some(identity_a_messages)
    );
    assert_eq!(
        load_response_session_messages(&pool, "identity-b", "resp-shared")
            .await
            .expect("load identity B response session"),
        Some(identity_b_messages)
    );
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
            original: std::env::var_os("PRELAY_MASTER_KEY"),
        }
    }
}

impl Drop for MasterKeyEnvironmentRestore {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var("PRELAY_MASTER_KEY", value),
            None => std::env::remove_var("PRELAY_MASTER_KEY"),
        }
    }
}

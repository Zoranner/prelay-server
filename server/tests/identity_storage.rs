use provider_relay_protocol::{CreateProviderRequest, ProtocolErrorCode};
use provider_relay_server::{
    db,
    storage::{MasterKey, Storage, StorageError},
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
    let registered = storage
        .register_identity("machine-a", "S-1-5-21-100")
        .await
        .expect("register identity");

    assert!(storage
        .authenticate_identity(&registered.credential)
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
        registered.credential
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
async fn credential_rotation_rejects_a_stale_authenticated_credential_hash() {
    let storage = test_storage().await;
    let registered = storage
        .register_identity("machine-a", "S-1-5-21-100")
        .await
        .expect("register identity");
    let credential_hash = storage
        .identity_credential_hash(&registered.identity_id)
        .await
        .expect("load authenticated credential hash");

    let rotated = storage
        .rotate_identity_credential(&registered.identity_id, &credential_hash)
        .await
        .expect("rotate with the authenticated credential hash");
    let stale_rotation = storage
        .rotate_identity_credential(&registered.identity_id, &credential_hash)
        .await
        .expect_err("reject stale authenticated credential hash");

    assert_eq!(stale_rotation.code(), ProtocolErrorCode::InvalidCredential);
    assert!(storage
        .authenticate_identity(&rotated.credential)
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
    let identity_a = storage
        .register_identity("machine-a", "S-1-5-21-100")
        .await
        .expect("register identity A");
    let identity_b = storage
        .register_identity("machine-b", "S-1-5-21-200")
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
async fn stable_key_cannot_reissue_a_lost_credential() {
    let storage = test_storage().await;
    storage
        .register_identity("machine-a", "S-1-5-21-100")
        .await
        .expect("register identity");

    let error = storage
        .register_identity("machine-a", "S-1-5-21-100")
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

    std::env::remove_var("PROVIDER_RELAY_MASTER_KEY");
    assert!(MasterKey::from_environment().is_err());

    std::env::set_var("PROVIDER_RELAY_MASTER_KEY", "not base64");
    assert!(MasterKey::from_environment().is_err());

    std::env::set_var("PROVIDER_RELAY_MASTER_KEY", "AAAA");
    assert!(MasterKey::from_environment().is_err());
}

#[tokio::test]
async fn identity_storage_schema_is_separate_from_legacy_v1_schema() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create sqlite pool");
    let storage = Storage::initialize(pool.clone(), MasterKey::from_bytes([0; 32]))
        .await
        .expect("initialize identity storage");

    db::init_schema(&pool)
        .await
        .expect("initialize legacy v1 schema");
    sqlx::query(
        "INSERT INTO provider_configs (id, name, provider_type, base_url, api_key, token, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("legacy-provider")
    .bind("Legacy provider")
    .bind("openai_compatible")
    .bind("https://legacy.example")
    .bind("sk-legacy")
    .bind("legacy-token")
    .bind("2026-08-13T00:00:00Z")
    .execute(&pool)
    .await
    .expect("create legacy v1 provider");

    let identity = storage
        .register_identity("machine-a", "S-1-5-21-100")
        .await
        .expect("register identity");
    let secure_provider_id = storage
        .create_provider(&identity.identity_id, provider_input("sk-secure"))
        .await
        .expect("create encrypted provider");

    let legacy_api_key = sqlx::query_scalar::<_, String>(
        "SELECT api_key FROM provider_configs WHERE id = 'legacy-provider'",
    )
    .fetch_one(&pool)
    .await
    .expect("read legacy v1 provider key");
    assert_eq!(legacy_api_key, "sk-legacy");
    assert_eq!(
        storage
            .decrypt_provider_key(&identity.identity_id, &secure_provider_id)
            .await
            .expect("decrypt encrypted provider key"),
        "sk-secure"
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
            original: std::env::var_os("PROVIDER_RELAY_MASTER_KEY"),
        }
    }
}

impl Drop for MasterKeyEnvironmentRestore {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var("PROVIDER_RELAY_MASTER_KEY", value),
            None => std::env::remove_var("PROVIDER_RELAY_MASTER_KEY"),
        }
    }
}

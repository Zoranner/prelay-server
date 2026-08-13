use provider_relay_protocol::{CreateProviderRequest, ProtocolErrorCode};
use provider_relay_server::storage::{MasterKey, Storage, StorageError};

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
            .raw_provider_key_ciphertext(&provider_id)
            .await
            .expect("load provider ciphertext"),
        "sk-secret"
    );
    assert_eq!(
        storage
            .decrypt_provider_key(&provider_id)
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
            .raw_provider_key_ciphertext(&provider_id)
            .await
            .expect("load first provider ciphertext"),
        storage
            .raw_provider_key_ciphertext(&second_provider_id)
            .await
            .expect("load second provider ciphertext")
    );
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

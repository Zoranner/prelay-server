use prelay_protocol::ProtocolErrorCode;
use prelay_server::{identity::credential::generate_credential, storage::StorageError};

use crate::support;

use super::fixtures::provider_input;

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

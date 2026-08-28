use prelay_server::{
    bridge::internal::InternalRole,
    storage::{ResponseSessionInsert, StorageError},
};

use crate::support;

use super::fixtures::{message, response, seed_identity_and_provider};

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

    let input_a = vec![message(InternalRole::User, "identity-a input")];
    let input_b = vec![message(InternalRole::User, "identity-b input")];
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
            message(InternalRole::User, "identity-a input"),
            message(InternalRole::Assistant, "identity-a output"),
        ])
    );
    assert_eq!(
        storage
            .load_response_session_messages(&identity_b, "shared-response")
            .await
            .expect("load identity B session"),
        Some(vec![
            message(InternalRole::User, "identity-b input"),
            message(InternalRole::Assistant, "identity-b output"),
        ])
    );
}

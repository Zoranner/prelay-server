use prelay_server::store::storage::{ResponseStateInsert, StorageError};
use serde_json::json;

use crate::support;

use super::fixtures::{response, seed_identity_and_provider};

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

    let request_a = json!([{ "role": "user", "content": "identity-a input" }]);
    let request_b = json!([{ "role": "user", "content": "identity-b input" }]);
    let response_items_a = json!([{
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": "identity-a output" }]
    }]);
    let response_items_b = json!([{
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": "identity-b output" }]
    }]);
    storage
        .save_response_state(ResponseStateInsert {
            identity_id: &identity_a,
            response_id: "shared-response",
            previous_response_id: None,
            conversation_id: "conversation-a",
            provider_id: &provider_a,
            model: "test-model",
            upstream_response_id: None,
            request_items: &request_a,
            response_items: &response_items_a,
            input_messages: &[],
            response: &response("response-a", "identity-a output"),
        })
        .await
        .expect("save identity A session");
    storage
        .save_response_state(ResponseStateInsert {
            identity_id: &identity_b,
            response_id: "shared-response",
            previous_response_id: None,
            conversation_id: "conversation-b",
            provider_id: &provider_b,
            model: "test-model",
            upstream_response_id: None,
            request_items: &request_b,
            response_items: &response_items_b,
            input_messages: &[],
            response: &response("response-b", "identity-b output"),
        })
        .await
        .expect("save identity B session");

    let state_a = storage
        .load_response_state(&identity_a, "shared-response")
        .await
        .expect("load identity A state")
        .expect("identity A state exists");
    assert_eq!(state_a.request_items, request_a);
    assert_eq!(state_a.response_items, response_items_a);

    let state_b = storage
        .load_response_state(&identity_b, "shared-response")
        .await
        .expect("load identity B state")
        .expect("identity B state exists");
    assert_eq!(state_b.request_items, request_b);
    assert_eq!(state_b.response_items, response_items_b);
}

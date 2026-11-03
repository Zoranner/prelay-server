use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
use serde_json::json;

use super::{response, seed_identity_and_provider, test_storage};
use crate::{
    store::entity::identity::response_sessions as identity_response_sessions,
    store::storage::ResponseStateInsert,
};

#[tokio::test]
async fn deletes_only_expired_response_states() {
    let storage = test_storage().await;
    let (identity_id, provider_id) = seed_identity_and_provider(&storage, "ttl").await;

    identity_response_sessions::ActiveModel {
        response_id: Set("resp_expired".to_string()),
        identity_id: Set(identity_id.clone()),
        previous_response_id: Set(None),
        conversation_id: Set(Some("conv_ttl".to_string())),
        provider_id: Set(provider_id.clone()),
        model: Set("model-ttl".to_string()),
        input_messages_json: Set("[]".to_string()),
        output_items_json: Set("[]".to_string()),
        status: Set("completed".to_string()),
        protocol_format: Set("responses_items_v1".to_string()),
        upstream_response_id: Set(Some("resp_upstream_expired".to_string())),
        request_items_json: Set(Some("[]".to_string())),
        response_items_json: Set(Some("[]".to_string())),
        expires_at: Set(Some("2000-01-01T00:00:00Z".to_string())),
        created_at: Set("2000-01-01T00:00:00Z".to_string()),
    }
    .insert(&storage.db)
    .await
    .expect("insert expired state");

    storage
        .save_response_state(ResponseStateInsert {
            identity_id: &identity_id,
            response_id: "resp_active",
            previous_response_id: None,
            conversation_id: "conv_ttl",
            provider_id: &provider_id,
            model: "model-ttl",
            upstream_response_id: None,
            request_items: &json!([]),
            response_items: &json!([]),
            input_messages: &[],
            response: &response("resp_active", "active"),
        })
        .await
        .expect("save active state");

    identity_response_sessions::ActiveModel {
        response_id: Set("resp_legacy".to_string()),
        identity_id: Set(identity_id.clone()),
        previous_response_id: Set(None),
        conversation_id: Set(None),
        provider_id: Set(provider_id),
        model: Set("model-ttl".to_string()),
        input_messages_json: Set("[]".to_string()),
        output_items_json: Set("[]".to_string()),
        status: Set("legacy".to_string()),
        protocol_format: Set("legacy_internal_v1".to_string()),
        upstream_response_id: Set(None),
        request_items_json: Set(None),
        response_items_json: Set(None),
        expires_at: Set(None),
        created_at: Set("2000-01-01T00:00:00Z".to_string()),
    }
    .insert(&storage.db)
    .await
    .expect("insert legacy state");

    let deleted = storage
        .delete_expired_response_states(Utc::now())
        .await
        .expect("delete expired states");
    assert_eq!(deleted, 1);

    let mut ids = identity_response_sessions::Entity::find()
        .all(&storage.db)
        .await
        .expect("load remaining states")
        .into_iter()
        .map(|row| row.response_id)
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, vec!["resp_active", "resp_legacy"]);
}

#[tokio::test]
async fn deleting_a_provider_does_not_block_response_state_history() {
    let storage = test_storage().await;
    let (identity_id, provider_id) = seed_identity_and_provider(&storage, "deleted").await;

    storage
        .save_response_state(ResponseStateInsert {
            identity_id: &identity_id,
            response_id: "resp_history",
            previous_response_id: None,
            conversation_id: "conv_deleted",
            provider_id: &provider_id,
            model: "model-deleted",
            upstream_response_id: None,
            request_items: &json!([]),
            response_items: &json!([]),
            input_messages: &[],
            response: &response("resp_history", "history"),
        })
        .await
        .expect("save state");

    storage
        .delete_provider(&identity_id, &provider_id)
        .await
        .expect("delete provider");

    let state = storage
        .load_response_state(&identity_id, "resp_history")
        .await
        .expect("load state after provider deletion")
        .expect("history remains");
    assert_eq!(state.provider_id, provider_id);
}

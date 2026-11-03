use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

use super::{seed_identity_and_provider, test_storage};
use crate::store::entity::identity::response_sessions as identity_response_sessions;

#[tokio::test]
async fn encrypts_legacy_response_session_content_once() {
    let storage = test_storage().await;
    let (identity_id, provider_id) = seed_identity_and_provider(&storage, "legacy-encrypt").await;

    identity_response_sessions::ActiveModel {
        response_id: Set("resp_legacy_plain".to_string()),
        identity_id: Set(identity_id.clone()),
        previous_response_id: Set(None),
        conversation_id: Set(None),
        provider_id: Set(provider_id),
        model: Set("model-legacy".to_string()),
        status: Set("legacy".to_string()),
        protocol_format: Set("legacy_internal_v1".to_string()),
        upstream_response_id: Set(None),
        request_items_json: Set(Some(r#"[{"secret":"request items"}]"#.to_string())),
        response_items_json: Set(Some(r#"[{"secret":"response items"}]"#.to_string())),
        expires_at: Set(None),
        input_messages_json: Set(r#"[{"secret":"input messages"}]"#.to_string()),
        output_items_json: Set(r#"[{"secret":"output items"}]"#.to_string()),
        created_at: Set("2026-01-01T00:00:00Z".to_string()),
    }
    .insert(&storage.db)
    .await
    .expect("insert legacy session");

    let migrated = storage
        .encrypt_legacy_response_sessions()
        .await
        .expect("encrypt legacy sessions");
    assert_eq!(migrated, 1);

    let row = identity_response_sessions::Entity::find()
        .filter(identity_response_sessions::Column::ResponseId.eq("resp_legacy_plain"))
        .filter(identity_response_sessions::Column::IdentityId.eq(&identity_id))
        .one(&storage.db)
        .await
        .expect("load migrated legacy session")
        .expect("legacy session exists");
    for value in [
        row.input_messages_json.as_str(),
        row.output_items_json.as_str(),
        row.request_items_json.as_deref().expect("request items"),
        row.response_items_json.as_deref().expect("response items"),
    ] {
        assert!(value.starts_with("enc:v1:"));
        assert!(!value.contains("secret"));
        assert!(!value.contains("input messages"));
        assert!(!value.contains("output items"));
    }
    assert_eq!(
        storage
            .encrypt_legacy_response_sessions()
            .await
            .expect("repeat legacy encryption"),
        0
    );
    assert!(storage
        .load_response_state(&identity_id, "resp_legacy_plain")
        .await
        .expect("load legacy state")
        .is_none());
}

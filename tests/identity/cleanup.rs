use chrono::{Duration, Utc};
use prelay_protocol::{CreateEndpointRequest, CreateProviderRequest, EndpointModelInput};
use prelay_server::{
    activity::ActivityContentDraft,
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalResponse, InternalRole,
    },
    identity::credential::generate_credential,
    stats::ActivityInsert,
    storage::{ResponseSessionInsert, Storage, StorageError},
};

use crate::support;

#[tokio::test]
async fn cleanup_removes_an_inactive_identity_and_all_accessible_owned_resources() {
    let storage = support::test_storage().await;
    let credential = generate_credential();
    let identity = storage
        .register_identity("machine-inactive", "S-1-5-21-100", &credential)
        .await
        .expect("register identity");
    let (provider_id, endpoint_id) =
        seed_owned_resources(&storage, &identity.identity_id, "inactive").await;
    let activity_id = storage
        .list_activities(&identity.identity_id, 1)
        .await
        .expect("load inactive activity")
        .pop()
        .expect("inactive activity")
        .id;
    storage
        .enqueue_activity_content(ActivityContentDraft {
            activity_id: activity_id.clone(),
            input_text: "short-lived body".to_string(),
            output_text: String::new(),
            media_metadata_json: None,
            is_truncated: false,
            content_hash: "inactive-content".to_string(),
        })
        .await
        .expect("store inactive activity content");

    assert_eq!(
        storage
            .delete_inactive_identities(Utc::now() + Duration::days(91), Duration::days(90))
            .await
            .expect("clean inactive identities"),
        1
    );
    assert!(storage
        .authenticate_identity(&credential)
        .await
        .expect("authenticate deleted identity")
        .is_none());
    assert!(storage
        .list_providers(&identity.identity_id)
        .await
        .expect("list deleted identity providers")
        .is_empty());
    assert!(storage
        .list_endpoints(&identity.identity_id)
        .await
        .expect("list deleted identity endpoints")
        .is_empty());
    assert!(storage
        .list_activities(&identity.identity_id, 10)
        .await
        .expect("list deleted identity activities")
        .is_empty());
    assert!(storage
        .find_activity_content(&activity_id)
        .await
        .expect("load deleted activity content")
        .is_none());
    assert_eq!(
        storage
            .load_response_session_messages(&identity.identity_id, "response-inactive")
            .await
            .expect("load deleted response session"),
        None
    );
    assert!(matches!(
        storage
            .get_provider(&identity.identity_id, &provider_id)
            .await,
        Err(StorageError::ProviderNotFound)
    ));
    assert!(matches!(
        storage
            .get_interface(&identity.identity_id, &endpoint_id)
            .await,
        Err(StorageError::EndpointNotFound)
    ));
}

#[tokio::test]
async fn cleanup_keeps_an_identity_that_is_newer_than_its_cutoff() {
    let storage = support::test_storage().await;
    let identity = storage
        .register_identity("machine-active", "S-1-5-21-200", &generate_credential())
        .await
        .expect("register identity");
    let (provider_id, endpoint_id) =
        seed_owned_resources(&storage, &identity.identity_id, "active").await;

    assert_eq!(
        storage
            .delete_inactive_identities(Utc::now() - Duration::days(1), Duration::zero())
            .await
            .expect("clean identities before their activity"),
        0
    );
    assert_eq!(
        storage
            .list_providers(&identity.identity_id)
            .await
            .expect("list active providers")
            .len(),
        1
    );
    assert_eq!(
        storage
            .list_endpoints(&identity.identity_id)
            .await
            .expect("list active endpoints")
            .len(),
        1
    );
    assert!(storage
        .get_provider(&identity.identity_id, &provider_id)
        .await
        .is_ok());
    assert!(storage
        .get_interface(&identity.identity_id, &endpoint_id)
        .await
        .is_ok());
    assert_eq!(
        storage
            .list_activities(&identity.identity_id, 10)
            .await
            .expect("list active activities")
            .len(),
        1
    );
    assert!(storage
        .load_response_session_messages(&identity.identity_id, "response-active")
        .await
        .expect("load active response session")
        .is_some());
}

async fn seed_owned_resources(
    storage: &Storage,
    identity_id: &str,
    suffix: &str,
) -> (String, String) {
    let provider_id = storage
        .create_provider(
            identity_id,
            CreateProviderRequest {
                name: "Test provider".to_string(),
                provider_type: "openai_compatible".to_string(),
                base_url: "https://provider.example".to_string(),
                api_key: "test-provider-key".to_string(),
                capabilities: None,
                models: vec!["upstream-model".to_string()],
            },
        )
        .await
        .expect("create provider");
    let endpoint = storage
        .create_interface(
            identity_id,
            CreateEndpointRequest {
                name: "Test endpoint".to_string(),
                protocol: None,
                models: vec![EndpointModelInput {
                    provider_id: provider_id.clone(),
                    upstream_model: "upstream-model".to_string(),
                }],
            },
        )
        .await
        .expect("create endpoint");
    storage
        .insert_activity_with_id(
            identity_id,
            format!("log-{suffix}"),
            ActivityInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                endpoint_name: endpoint.name.clone(),
                provider_id: provider_id.clone(),
                provider_name: "Test provider".to_string(),
                model_requested: "upstream-model".to_string(),
                model_upstream: "upstream-model".to_string(),
                status: "success".to_string(),
                http_status: 200,
                latency_ms: 1,
                ..Default::default()
            },
        )
        .await
        .expect("create activity");
    let input = vec![InternalMessage {
        role: InternalRole::User,
        content: vec![InternalContentPart::Text("input".to_string())],
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    }];
    let response = InternalResponse {
        id: format!("response-{suffix}"),
        model: "upstream-model".to_string(),
        output: vec![InternalOutputItem::Message {
            id: format!("message-{suffix}"),
            role: InternalRole::Assistant,
            content: vec![InternalContentPart::Text("output".to_string())],
        }],
        usage: None,
    };
    storage
        .save_response_session(ResponseSessionInsert {
            identity_id,
            response_id: &response.id,
            previous_response_id: None,
            provider_id: &provider_id,
            model: "upstream-model",
            input_messages: &input,
            response: &response,
        })
        .await
        .expect("create response session");
    (provider_id, endpoint.id)
}

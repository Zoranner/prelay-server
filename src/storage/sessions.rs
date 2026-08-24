use chrono::Utc;
use sea_orm::{
    sea_query::OnConflict, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter,
};

use crate::bridge::internal::{
    InternalMessage, InternalOutputItem, InternalResponse, InternalToolCall,
};
use crate::{entity::identity_response_sessions, storage::StorageError};

pub struct ResponseSessionInsert<'a> {
    pub identity_id: &'a str,
    pub response_id: &'a str,
    pub previous_response_id: Option<&'a str>,
    pub provider_id: &'a str,
    pub model: &'a str,
    pub input_messages: &'a [InternalMessage],
    pub response: &'a InternalResponse,
}

pub(super) async fn save_response_session(
    db: &DatabaseConnection,
    insert: ResponseSessionInsert<'_>,
) -> Result<(), StorageError> {
    let session = identity_response_sessions::ActiveModel {
        response_id: Set(insert.response_id.to_string()),
        identity_id: Set(insert.identity_id.to_string()),
        previous_response_id: Set(insert.previous_response_id.map(str::to_string)),
        provider_id: Set(insert.provider_id.to_string()),
        model: Set(insert.model.to_string()),
        input_messages_json: Set(serde_json::to_string(insert.input_messages)?),
        output_items_json: Set(serde_json::to_string(&insert.response.output)?),
        created_at: Set(Utc::now().to_rfc3339()),
    };
    identity_response_sessions::Entity::insert(session)
        .on_conflict(
            OnConflict::columns([
                identity_response_sessions::Column::ResponseId,
                identity_response_sessions::Column::IdentityId,
            ])
            .update_columns([
                identity_response_sessions::Column::PreviousResponseId,
                identity_response_sessions::Column::ProviderId,
                identity_response_sessions::Column::Model,
                identity_response_sessions::Column::InputMessagesJson,
                identity_response_sessions::Column::OutputItemsJson,
                identity_response_sessions::Column::CreatedAt,
            ])
            .to_owned(),
        )
        .exec(db)
        .await?;
    Ok(())
}

pub(super) async fn load_response_session_messages(
    db: &DatabaseConnection,
    identity_id: &str,
    response_id: &str,
) -> Result<Option<Vec<InternalMessage>>, StorageError> {
    let row = identity_response_sessions::Entity::find()
        .filter(identity_response_sessions::Column::ResponseId.eq(response_id))
        .filter(identity_response_sessions::Column::IdentityId.eq(identity_id))
        .one(db)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let mut messages: Vec<InternalMessage> = serde_json::from_str(&row.input_messages_json)?;
    let items: Vec<InternalOutputItem> = serde_json::from_str(&row.output_items_json)?;
    messages.extend(items.into_iter().map(output_item_to_message));
    Ok(Some(messages))
}

fn output_item_to_message(item: InternalOutputItem) -> InternalMessage {
    match item {
        InternalOutputItem::Message { role, content, .. } => InternalMessage {
            role,
            content,
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
        },
        InternalOutputItem::FunctionToolCall {
            id,
            name,
            arguments,
            reasoning_content,
        } => InternalMessage {
            role: crate::bridge::internal::InternalRole::Assistant,
            content: Vec::new(),
            tool_call_id: None,
            tool_calls: vec![InternalToolCall {
                id,
                name,
                arguments,
            }],
            reasoning_content,
        },
    }
}

#[cfg(test)]
mod tests {
    use prelay_protocol::CreateProviderRequest;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, Database};

    use super::ResponseSessionInsert;
    use crate::{
        bridge::internal::{
            InternalContentPart, InternalMessage, InternalOutputItem, InternalResponse,
            InternalRole,
        },
        entity::identity_response_sessions,
        migration::apply_all,
        storage::{MasterKey, Storage},
    };

    #[tokio::test]
    async fn response_sessions_with_the_same_id_stay_isolated_by_identity() {
        let storage = test_storage().await;
        let (identity_a, provider_a) = seed_identity_and_provider(&storage, "a").await;
        let (identity_b, provider_b) = seed_identity_and_provider(&storage, "b").await;

        let input_a = vec![message("identity-a input")];
        let input_b = vec![message("identity-b input")];
        let response_a = response("shared-response", "identity-a output");
        let response_b = response("shared-response", "identity-b output");
        storage
            .save_response_session(ResponseSessionInsert {
                identity_id: &identity_a,
                response_id: "shared-response",
                previous_response_id: None,
                provider_id: &provider_a,
                model: "model-a",
                input_messages: &input_a,
                response: &response_a,
            })
            .await
            .expect("save identity A session");
        storage
            .save_response_session(ResponseSessionInsert {
                identity_id: &identity_b,
                response_id: "shared-response",
                previous_response_id: None,
                provider_id: &provider_b,
                model: "model-b",
                input_messages: &input_b,
                response: &response_b,
            })
            .await
            .expect("save identity B session");

        let updated_input_a = vec![message("identity-a updated input")];
        let updated_response_a = response("shared-response", "identity-a updated output");
        storage
            .save_response_session(ResponseSessionInsert {
                identity_id: &identity_a,
                response_id: "shared-response",
                previous_response_id: Some("previous-a"),
                provider_id: &provider_a,
                model: "model-a-updated",
                input_messages: &updated_input_a,
                response: &updated_response_a,
            })
            .await
            .expect("upsert identity A session");

        assert_eq!(
            storage
                .load_response_session_messages(&identity_a, "shared-response")
                .await
                .expect("load identity A session"),
            Some(vec![
                message("identity-a updated input"),
                assistant_message("identity-a updated output"),
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
    async fn loads_legacy_session_json_without_reasoning_content() {
        let storage = test_storage().await;
        let (identity_id, provider_id) = seed_identity_and_provider(&storage, "legacy").await;

        identity_response_sessions::ActiveModel {
            response_id: Set("resp_old".to_string()),
            identity_id: Set(identity_id.clone()),
            previous_response_id: Set(None),
            provider_id: Set(provider_id),
            model: Set("deepseek-chat".to_string()),
            input_messages_json: Set(
                r#"[{"role":"User","content":[{"Text":"read Cargo.toml"}],"tool_call_id":null,"tool_calls":[]}]"#
                    .to_string(),
            ),
            output_items_json: Set(
                r#"[{"FunctionToolCall":{"id":"call_1","name":"read_file","arguments":"{\"path\":\"Cargo.toml\"}"}}]"#
                    .to_string(),
            ),
            created_at: Set("2026-01-01T00:00:00Z".to_string()),
        }
        .insert(&storage.db)
        .await
        .expect("insert legacy session");

        let messages = storage
            .load_response_session_messages(&identity_id, "resp_old")
            .await
            .expect("load legacy session")
            .expect("legacy session exists");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].reasoning_content, None);
        assert_eq!(messages[1].reasoning_content, None);
        assert_eq!(messages[1].tool_calls[0].id, "call_1");
    }

    async fn test_storage() -> Storage {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect test database");
        apply_all(&db).await.expect("apply test migrations");
        Storage::from_connection(db, MasterKey::from_bytes([0; 32]))
    }

    async fn seed_identity_and_provider(storage: &Storage, suffix: &str) -> (String, String) {
        let identity = storage
            .register_identity(
                &format!("machine-{suffix}"),
                &format!("sid-{suffix}"),
                &crate::identity::credential::generate_credential(),
            )
            .await
            .expect("register identity");
        let provider_id = storage
            .create_provider(
                &identity.identity_id,
                CreateProviderRequest {
                    name: format!("Provider {suffix}"),
                    provider_type: "openai_compatible".to_string(),
                    base_url: "https://provider.example".to_string(),
                    api_key: format!("key-{suffix}"),
                    capabilities: None,
                    models: vec![format!("model-{suffix}")],
                },
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
}

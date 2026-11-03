use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sea_orm::{
    sea_query::OnConflict, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter,
};
use serde_json::Value;

use crate::{
    codec::internal::{InternalMessage, InternalResponse},
    store::entity::identity::response_sessions as identity_response_sessions,
    store::storage::{
        crypto::KeyCipher,
        sessions::{decrypt_session_value, encrypt_session_value},
        Storage, StorageError,
    },
};

#[derive(Clone, Debug, PartialEq)]
pub struct ResponseState {
    pub identity_id: String,
    pub response_id: String,
    pub previous_response_id: Option<String>,
    pub conversation_id: Option<String>,
    pub provider_id: String,
    pub model: String,
    pub upstream_response_id: Option<String>,
    pub request_items: Value,
    pub response_items: Value,
    pub created_at: String,
    pub expires_at: String,
}

pub struct ResponseStateInsert<'a> {
    pub identity_id: &'a str,
    pub response_id: &'a str,
    pub previous_response_id: Option<&'a str>,
    pub conversation_id: &'a str,
    pub provider_id: &'a str,
    pub model: &'a str,
    pub upstream_response_id: Option<&'a str>,
    pub request_items: &'a Value,
    pub response_items: &'a Value,
    pub input_messages: &'a [InternalMessage],
    pub response: &'a InternalResponse,
}

impl Storage {
    pub async fn save_response_state(
        &self,
        insert: ResponseStateInsert<'_>,
    ) -> Result<(), StorageError> {
        save_response_state(&self.db, &self.crypto, insert).await
    }

    pub async fn load_response_state(
        &self,
        identity_id: &str,
        response_id: &str,
    ) -> Result<Option<ResponseState>, StorageError> {
        load_response_state(&self.db, &self.crypto, identity_id, response_id).await
    }

    pub async fn delete_expired_response_states(
        &self,
        now: DateTime<Utc>,
    ) -> Result<u64, StorageError> {
        delete_expired_response_states(&self.db, now).await
    }
}

async fn save_response_state(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    insert: ResponseStateInsert<'_>,
) -> Result<(), StorageError> {
    let now = Utc::now();
    let created_at = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let expires_at = (now + Duration::days(30)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let request_items_json =
        encrypt_session_value(crypto, &serde_json::to_string(insert.request_items)?)?;
    let response_items_json =
        encrypt_session_value(crypto, &serde_json::to_string(insert.response_items)?)?;
    let input_messages_json =
        encrypt_session_value(crypto, &serde_json::to_string(insert.input_messages)?)?;
    let output_items_json =
        encrypt_session_value(crypto, &serde_json::to_string(&insert.response.output)?)?;

    let state = identity_response_sessions::ActiveModel {
        response_id: Set(insert.response_id.to_string()),
        identity_id: Set(insert.identity_id.to_string()),
        previous_response_id: Set(insert.previous_response_id.map(str::to_string)),
        conversation_id: Set(Some(insert.conversation_id.to_string())),
        provider_id: Set(insert.provider_id.to_string()),
        model: Set(insert.model.to_string()),
        status: Set("completed".to_string()),
        protocol_format: Set("responses_items_v1".to_string()),
        upstream_response_id: Set(insert.upstream_response_id.map(str::to_string)),
        request_items_json: Set(Some(request_items_json)),
        response_items_json: Set(Some(response_items_json)),
        expires_at: Set(Some(expires_at)),
        input_messages_json: Set(input_messages_json),
        output_items_json: Set(output_items_json),
        created_at: Set(created_at),
    };
    identity_response_sessions::Entity::insert(state)
        .on_conflict(
            OnConflict::columns([
                identity_response_sessions::Column::ResponseId,
                identity_response_sessions::Column::IdentityId,
            ])
            .update_columns([
                identity_response_sessions::Column::PreviousResponseId,
                identity_response_sessions::Column::ConversationId,
                identity_response_sessions::Column::ProviderId,
                identity_response_sessions::Column::Model,
                identity_response_sessions::Column::Status,
                identity_response_sessions::Column::ProtocolFormat,
                identity_response_sessions::Column::UpstreamResponseId,
                identity_response_sessions::Column::RequestItemsJson,
                identity_response_sessions::Column::ResponseItemsJson,
                identity_response_sessions::Column::ExpiresAt,
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

async fn load_response_state(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    response_id: &str,
) -> Result<Option<ResponseState>, StorageError> {
    let row = identity_response_sessions::Entity::find()
        .filter(identity_response_sessions::Column::ResponseId.eq(response_id))
        .filter(identity_response_sessions::Column::IdentityId.eq(identity_id))
        .one(db)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.protocol_format != "responses_items_v1" || row.status != "completed" {
        return Ok(None);
    }
    let expires_at = row.expires_at.ok_or_else(|| {
        StorageError::InvalidStoredState("completed response has no expiry".to_string())
    })?;
    let expiry = DateTime::parse_from_rfc3339(&expires_at).map_err(|error| {
        StorageError::InvalidStoredState(format!("invalid response expiry: {error}"))
    })?;
    if expiry.with_timezone(&Utc) <= Utc::now() {
        return Ok(None);
    }
    let request_items_json = row
        .request_items_json
        .ok_or_else(|| StorageError::InvalidStoredState("missing request items".to_string()))?;
    let response_items_json = row
        .response_items_json
        .ok_or_else(|| StorageError::InvalidStoredState("missing response items".to_string()))?;
    let request_items = serde_json::from_str(&decrypt_session_value(crypto, &request_items_json)?)
        .map_err(|error| {
            StorageError::InvalidStoredState(format!("invalid request items: {error}"))
        })?;
    let response_items =
        serde_json::from_str(&decrypt_session_value(crypto, &response_items_json)?).map_err(
            |error| StorageError::InvalidStoredState(format!("invalid response items: {error}")),
        )?;

    Ok(Some(ResponseState {
        identity_id: row.identity_id,
        response_id: row.response_id,
        previous_response_id: row.previous_response_id,
        conversation_id: row.conversation_id,
        provider_id: row.provider_id,
        model: row.model,
        upstream_response_id: row.upstream_response_id,
        request_items,
        response_items,
        created_at: row.created_at,
        expires_at,
    }))
}

async fn delete_expired_response_states(
    db: &DatabaseConnection,
    now: DateTime<Utc>,
) -> Result<u64, StorageError> {
    let now = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let result = identity_response_sessions::Entity::delete_many()
        .filter(identity_response_sessions::Column::ExpiresAt.is_not_null())
        .filter(identity_response_sessions::Column::ExpiresAt.lt(now))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use prelay_protocol::CreateProviderRequest;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
    use serde_json::json;

    use crate::{
        codec::internal::{
            InternalContentPart, InternalOutputItem, InternalResponse, InternalRole,
        },
        store::entity::identity::response_sessions as identity_response_sessions,
        store::storage::{ResponseStateInsert, Storage},
    };

    #[tokio::test]
    async fn stores_and_loads_exact_response_items() {
        let storage = test_storage().await;
        let (identity_id, provider_id) = seed_identity_and_provider(&storage, "exact").await;
        let request_items = json!([
            {"role": "user", "content": "hello"}
        ]);
        let response_items = json!([
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "hi"}]
            }
        ]);

        storage
            .save_response_state(ResponseStateInsert {
                identity_id: &identity_id,
                response_id: "resp_local",
                previous_response_id: None,
                conversation_id: "conv_local",
                provider_id: &provider_id,
                model: "model-local",
                upstream_response_id: Some("resp_upstream"),
                request_items: &request_items,
                response_items: &response_items,
                input_messages: &[],
                response: &response("resp_local", "hi"),
            })
            .await
            .expect("save exact state");

        let state = storage
            .load_response_state(&identity_id, "resp_local")
            .await
            .expect("load exact state")
            .expect("state exists");
        assert_eq!(state.request_items, request_items);
        assert_eq!(state.response_items, response_items);
        assert_eq!(state.upstream_response_id.as_deref(), Some("resp_upstream"));
    }

    #[tokio::test]
    async fn encrypts_exact_response_items() {
        let storage = test_storage().await;
        let (identity_id, provider_id) = seed_identity_and_provider(&storage, "encrypted").await;

        storage
            .save_response_state(ResponseStateInsert {
                identity_id: &identity_id,
                response_id: "resp_encrypted",
                previous_response_id: None,
                conversation_id: "conv_encrypted",
                provider_id: &provider_id,
                model: "model-encrypted",
                upstream_response_id: None,
                request_items: &json!([{"secret": "request"}]),
                response_items: &json!([{"secret": "response"}]),
                input_messages: &[],
                response: &response("resp_encrypted", "encrypted"),
            })
            .await
            .expect("save exact state");

        let row = identity_response_sessions::Entity::find()
            .filter(identity_response_sessions::Column::ResponseId.eq("resp_encrypted"))
            .filter(identity_response_sessions::Column::IdentityId.eq(&identity_id))
            .one(&storage.db)
            .await
            .expect("load stored row")
            .expect("stored row");
        assert!(row
            .request_items_json
            .as_deref()
            .is_some_and(|value| value.starts_with("enc:v1:")));
        assert!(row
            .response_items_json
            .as_deref()
            .is_some_and(|value| value.starts_with("enc:v1:")));
        assert!(!row
            .request_items_json
            .as_deref()
            .unwrap_or_default()
            .contains("request"));
        assert!(!row
            .response_items_json
            .as_deref()
            .unwrap_or_default()
            .contains("response"));
    }

    #[tokio::test]
    async fn does_not_load_legacy_or_expired_state() {
        let storage = test_storage().await;
        let (identity_id, provider_id) = seed_identity_and_provider(&storage, "legacy").await;

        identity_response_sessions::ActiveModel {
            response_id: Set("resp_legacy".to_string()),
            identity_id: Set(identity_id.clone()),
            previous_response_id: Set(None),
            conversation_id: Set(None),
            provider_id: Set(provider_id.clone()),
            model: Set("model".to_string()),
            input_messages_json: Set("[]".to_string()),
            output_items_json: Set("[]".to_string()),
            status: Set("legacy".to_string()),
            protocol_format: Set("legacy_internal_v1".to_string()),
            upstream_response_id: Set(None),
            request_items_json: Set(None),
            response_items_json: Set(None),
            expires_at: Set(None),
            created_at: Set("2026-01-01T00:00:00Z".to_string()),
        }
        .insert(&storage.db)
        .await
        .expect("insert legacy row");

        assert!(storage
            .load_response_state(&identity_id, "resp_legacy")
            .await
            .expect("load legacy")
            .is_none());

        identity_response_sessions::ActiveModel {
            response_id: Set("resp_expired".to_string()),
            identity_id: Set(identity_id.clone()),
            previous_response_id: Set(None),
            conversation_id: Set(None),
            provider_id: Set(provider_id),
            model: Set("model".to_string()),
            input_messages_json: Set("[]".to_string()),
            output_items_json: Set("[]".to_string()),
            status: Set("completed".to_string()),
            protocol_format: Set("responses_items_v1".to_string()),
            upstream_response_id: Set(None),
            request_items_json: Set(Some("[]".to_string())),
            response_items_json: Set(Some("[]".to_string())),
            expires_at: Set(Some("2000-01-01T00:00:00Z".to_string())),
            created_at: Set("2026-01-01T00:00:00Z".to_string()),
        }
        .insert(&storage.db)
        .await
        .expect("insert expired row");

        assert!(storage
            .load_response_state(&identity_id, "resp_expired")
            .await
            .expect("load expired")
            .is_none());
    }

    async fn test_storage() -> Storage {
        crate::test_support::test_storage().await
    }

    async fn seed_identity_and_provider(storage: &Storage, suffix: &str) -> (String, String) {
        let identity = storage
            .register_identity(
                &format!("machine-{suffix}"),
                &format!("sid-{suffix}"),
                &crate::domain::identity::credential::generate_credential(),
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
                    models: None,
                },
            )
            .await
            .expect("create provider");
        (identity.identity_id, provider_id)
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

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    IntoActiveModel, QueryFilter, QueryOrder, QuerySelect,
};

use super::identity_response_sessions;
use crate::store::storage::{
    crypto::KeyCipher,
    sessions::{encrypt_session_value, is_encrypted_session_value},
    StorageError,
};

const LEGACY_RESPONSE_BATCH: u64 = 100;

pub(super) async fn encrypt_legacy_response_sessions(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
) -> Result<u64, StorageError> {
    let mut offset = 0u64;
    let mut migrated = 0u64;
    loop {
        let rows = identity_response_sessions::Entity::find()
            .filter(identity_response_sessions::Column::ProtocolFormat.eq("legacy_internal_v1"))
            .order_by_asc(identity_response_sessions::Column::ResponseId)
            .order_by_asc(identity_response_sessions::Column::IdentityId)
            .limit(LEGACY_RESPONSE_BATCH)
            .offset(offset)
            .all(db)
            .await?;
        if rows.is_empty() {
            break;
        }
        offset += rows.len() as u64;
        for row in rows {
            let input = encrypt_legacy_value(crypto, &row.input_messages_json)?;
            let output = encrypt_legacy_value(crypto, &row.output_items_json)?;
            let request = encrypt_optional_legacy_value(crypto, row.request_items_json.as_deref())?;
            let response =
                encrypt_optional_legacy_value(crypto, row.response_items_json.as_deref())?;
            if input.is_none() && output.is_none() && request.is_none() && response.is_none() {
                continue;
            }
            let mut active = row.into_active_model();
            if let Some(value) = input {
                active.input_messages_json = Set(value);
            }
            if let Some(value) = output {
                active.output_items_json = Set(value);
            }
            if let Some(value) = request {
                active.request_items_json = Set(Some(value));
            }
            if let Some(value) = response {
                active.response_items_json = Set(Some(value));
            }
            active.update(db).await?;
            migrated += 1;
        }
    }
    Ok(migrated)
}

fn encrypt_legacy_value(crypto: &KeyCipher, value: &str) -> Result<Option<String>, StorageError> {
    if is_encrypted_session_value(value) {
        return Ok(None);
    }
    encrypt_session_value(crypto, value).map(Some)
}

fn encrypt_optional_legacy_value(
    crypto: &KeyCipher,
    value: Option<&str>,
) -> Result<Option<String>, StorageError> {
    value
        .filter(|value| !is_encrypted_session_value(value))
        .map(|value| encrypt_session_value(crypto, value))
        .transpose()
}

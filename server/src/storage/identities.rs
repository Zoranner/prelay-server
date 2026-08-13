use chrono::Utc;
use provider_relay_protocol::CreateIdentityResponse;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::identity::credential::{credential_hashes_match, generate_credential, hash_credential};

use super::StorageError;

pub(crate) async fn register(
    pool: &SqlitePool,
    machine_id: &str,
    account_sid: &str,
) -> Result<CreateIdentityResponse, StorageError> {
    let identity_id = Uuid::new_v4().to_string();
    let credential = generate_credential();
    let credential_hash = hash_credential(&credential);
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "INSERT INTO identities (id, machine_id, account_sid, credential_hash, created_at, last_active_at)\
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&identity_id)
    .bind(machine_id)
    .bind(account_sid)
    .bind(credential_hash)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await;
    match result {
        Ok(_) => Ok(CreateIdentityResponse {
            identity_id,
            credential,
        }),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            Err(StorageError::IdentityAlreadyRegistered)
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn authenticate(
    pool: &SqlitePool,
    credential: &str,
) -> Result<Option<String>, StorageError> {
    let supplied_hash = hash_credential(credential);
    let rows = sqlx::query_as::<_, (String, String)>("SELECT id, credential_hash FROM identities")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().find_map(|(id, expected_hash)| {
        credential_hashes_match(&expected_hash, &supplied_hash).then_some(id)
    }))
}

pub(crate) async fn credential_hash(
    pool: &SqlitePool,
    identity_id: &str,
) -> Result<String, StorageError> {
    sqlx::query_scalar("SELECT credential_hash FROM identities WHERE id = ?")
        .bind(identity_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

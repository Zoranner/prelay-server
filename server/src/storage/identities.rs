use chrono::Utc;
use provider_relay_protocol::{CreateIdentityResponse, RotateCredentialResponse};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::identity::credential::{credential_hashes_match, generate_credential, hash_credential};

use super::StorageError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedIdentity {
    pub id: String,
    pub credential_hash: String,
}

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
) -> Result<Option<AuthenticatedIdentity>, StorageError> {
    let supplied_hash = hash_credential(credential);
    let rows = sqlx::query_as::<_, (String, String)>("SELECT id, credential_hash FROM identities")
        .fetch_all(pool)
        .await?;
    let identity = rows.into_iter().find_map(|(id, credential_hash)| {
        credential_hashes_match(&credential_hash, &supplied_hash).then_some(AuthenticatedIdentity {
            id,
            credential_hash,
        })
    });
    if let Some(identity) = &identity {
        sqlx::query("UPDATE identities SET last_active_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(&identity.id)
            .execute(pool)
            .await?;
    }
    Ok(identity)
}

pub(crate) async fn rotate_credential(
    pool: &SqlitePool,
    identity_id: &str,
    authenticated_credential_hash: &str,
) -> Result<RotateCredentialResponse, StorageError> {
    let credential = generate_credential();
    let result = sqlx::query(
        "UPDATE identities SET credential_hash = ?, last_active_at = ? \
             WHERE id = ? AND credential_hash = ?",
    )
    .bind(hash_credential(&credential))
    .bind(Utc::now().to_rfc3339())
    .bind(identity_id)
    .bind(authenticated_credential_hash)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(StorageError::InvalidCredential);
    }
    Ok(RotateCredentialResponse { credential })
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

pub(crate) async fn touch(pool: &SqlitePool, identity_id: &str) -> Result<(), StorageError> {
    sqlx::query("UPDATE identities SET last_active_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(identity_id)
        .execute(pool)
        .await?;
    Ok(())
}

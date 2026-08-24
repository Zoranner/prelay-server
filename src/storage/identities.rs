use chrono::{DateTime, Duration, Utc};
use prelay_protocol::{CreateIdentityResponse, RotateCredentialResponse};
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection,
    EntityTrait, IntoActiveModel, QueryFilter, TransactionTrait,
};
use uuid::Uuid;

use crate::{
    entity::{
        identities, identity_endpoint_configs, identity_endpoint_model_routes,
        identity_endpoint_models, identity_model_aliases, identity_provider_configs,
        identity_provider_models, identity_request_logs, identity_response_sessions,
    },
    identity::credential::{credential_hashes_match, hash_credential, is_valid_device_credential},
};

use super::StorageError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedIdentity {
    pub id: String,
    pub credential_hash: String,
}

pub(crate) async fn register(
    db: &DatabaseConnection,
    machine_id: &str,
    account_sid: &str,
    credential: &str,
    display_name: Option<&str>,
) -> Result<CreateIdentityResponse, StorageError> {
    validate_device_credential(credential)?;
    let identity_id = Uuid::new_v4().to_string();
    let credential_hash = hash_credential(credential);
    let now = Utc::now().to_rfc3339();
    let identity = identities::ActiveModel {
        id: Set(identity_id.clone()),
        machine_id: Set(machine_id.to_string()),
        account_sid: Set(account_sid.to_string()),
        credential_hash: Set(credential_hash.clone()),
        display_name: Set(display_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
            .to_string()),
        created_at: Set(now.clone()),
        last_active_at: Set(now),
    };
    match identity.insert(db).await {
        Ok(_) => Ok(CreateIdentityResponse {
            identity_id,
            created: true,
        }),
        Err(error) => {
            let existing = identities::Entity::find()
                .filter(identities::Column::MachineId.eq(machine_id))
                .filter(identities::Column::AccountSid.eq(account_sid))
                .one(db)
                .await?;
            match existing {
                Some(existing)
                    if credential_hashes_match(&existing.credential_hash, &credential_hash) =>
                {
                    Ok(CreateIdentityResponse {
                        identity_id: existing.id,
                        created: false,
                    })
                }
                Some(_) => Err(StorageError::IdentityAlreadyRegistered),
                None => Err(StorageError::Database(error)),
            }
        }
    }
}

pub(crate) async fn authenticate(
    db: &DatabaseConnection,
    credential: &str,
    display_name: Option<&str>,
) -> Result<Option<AuthenticatedIdentity>, StorageError> {
    let supplied_hash = hash_credential(credential);
    let rows = identities::Entity::find().all(db).await?;
    let identity = rows
        .into_iter()
        .find(|identity| credential_hashes_match(&identity.credential_hash, &supplied_hash));
    let Some(identity) = identity else {
        return Ok(None);
    };

    let authenticated = AuthenticatedIdentity {
        id: identity.id.clone(),
        credential_hash: identity.credential_hash.clone(),
    };
    let mut active = identity.into_active_model();
    active.last_active_at = Set(Utc::now().to_rfc3339());
    if let Some(display_name) = display_name.filter(|value| !value.trim().is_empty()) {
        active.display_name = Set(display_name.trim().to_string());
    }
    active.update(db).await?;
    Ok(Some(authenticated))
}

pub(crate) async fn rotate_credential(
    db: &DatabaseConnection,
    identity_id: &str,
    authenticated_credential_hash: &str,
    new_credential: &str,
) -> Result<RotateCredentialResponse, StorageError> {
    validate_device_credential(new_credential)?;
    let result = identities::Entity::update_many()
        .col_expr(
            identities::Column::CredentialHash,
            Expr::value(hash_credential(new_credential)),
        )
        .col_expr(
            identities::Column::LastActiveAt,
            Expr::value(Utc::now().to_rfc3339()),
        )
        .filter(identities::Column::Id.eq(identity_id))
        .filter(identities::Column::CredentialHash.eq(authenticated_credential_hash))
        .exec(db)
        .await?;
    if result.rows_affected == 0 {
        return Err(StorageError::InvalidCredential);
    }
    Ok(RotateCredentialResponse { rotated: true })
}

fn validate_device_credential(credential: &str) -> Result<(), StorageError> {
    if is_valid_device_credential(credential) {
        return Ok(());
    }
    Err(StorageError::ValidationFailed(
        "device credential must be a 32-byte URL-safe Base64 value".to_string(),
    ))
}

pub(crate) async fn credential_hash(
    db: &DatabaseConnection,
    identity_id: &str,
) -> Result<String, StorageError> {
    identities::Entity::find_by_id(identity_id)
        .one(db)
        .await?
        .map(|identity| identity.credential_hash)
        .ok_or(StorageError::IdentityNotFound)
}

pub(crate) async fn touch(db: &DatabaseConnection, identity_id: &str) -> Result<(), StorageError> {
    identities::Entity::update_many()
        .col_expr(
            identities::Column::LastActiveAt,
            Expr::value(Utc::now().to_rfc3339()),
        )
        .filter(identities::Column::Id.eq(identity_id))
        .exec(db)
        .await?;
    Ok(())
}

pub(crate) async fn delete_inactive(
    db: &DatabaseConnection,
    now: DateTime<Utc>,
    retention: Duration,
) -> Result<u64, StorageError> {
    let cutoff = (now - retention).to_rfc3339();
    let transaction = db.begin().await?;
    let identity_ids = identities::Entity::find()
        .filter(identities::Column::LastActiveAt.lte(cutoff))
        .all(&transaction)
        .await?
        .into_iter()
        .map(|identity| identity.id)
        .collect::<Vec<_>>();
    if identity_ids.is_empty() {
        transaction.commit().await?;
        return Ok(0);
    }

    let endpoint_ids = identity_endpoint_configs::Entity::find()
        .filter(identity_endpoint_configs::Column::IdentityId.is_in(identity_ids.clone()))
        .all(&transaction)
        .await?
        .into_iter()
        .map(|endpoint| endpoint.id)
        .collect::<Vec<_>>();
    let provider_ids = identity_provider_configs::Entity::find()
        .filter(identity_provider_configs::Column::IdentityId.is_in(identity_ids.clone()))
        .all(&transaction)
        .await?
        .into_iter()
        .map(|provider| provider.id)
        .collect::<Vec<_>>();

    identity_request_logs::Entity::delete_many()
        .filter(identity_request_logs::Column::IdentityId.is_in(identity_ids.clone()))
        .exec(&transaction)
        .await?;
    identity_response_sessions::Entity::delete_many()
        .filter(identity_response_sessions::Column::IdentityId.is_in(identity_ids.clone()))
        .exec(&transaction)
        .await?;
    if !endpoint_ids.is_empty() {
        identity_endpoint_model_routes::Entity::delete_many()
            .filter(identity_endpoint_model_routes::Column::EndpointId.is_in(endpoint_ids.clone()))
            .exec(&transaction)
            .await?;
        identity_endpoint_models::Entity::delete_many()
            .filter(identity_endpoint_models::Column::EndpointId.is_in(endpoint_ids))
            .exec(&transaction)
            .await?;
    }
    identity_endpoint_configs::Entity::delete_many()
        .filter(identity_endpoint_configs::Column::IdentityId.is_in(identity_ids.clone()))
        .exec(&transaction)
        .await?;
    if !provider_ids.is_empty() {
        identity_provider_models::Entity::delete_many()
            .filter(identity_provider_models::Column::ProviderId.is_in(provider_ids))
            .exec(&transaction)
            .await?;
    }
    identity_model_aliases::Entity::delete_many()
        .filter(identity_model_aliases::Column::IdentityId.is_in(identity_ids.clone()))
        .exec(&transaction)
        .await?;
    identity_provider_configs::Entity::delete_many()
        .filter(identity_provider_configs::Column::IdentityId.is_in(identity_ids.clone()))
        .exec(&transaction)
        .await?;
    let deleted = identities::Entity::delete_many()
        .filter(identities::Column::Id.is_in(identity_ids))
        .exec(&transaction)
        .await?
        .rows_affected;
    transaction.commit().await?;
    Ok(deleted)
}

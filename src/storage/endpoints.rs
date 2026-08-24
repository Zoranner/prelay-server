use std::collections::HashSet;

use chrono::Utc;
use prelay_protocol::{
    CreateEndpointRequest, EndpointModelInput, EndpointModelResponse, EndpointResponse,
    UpdateEndpointRequest,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    DatabaseTransaction, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, TransactionTrait,
};
use uuid::Uuid;

use crate::{
    entity::{
        identities, identity_endpoint_configs, identity_endpoint_model_routes,
        identity_endpoint_models, identity_provider_configs, identity_provider_models,
    },
    identity::credential::generate_credential,
};

use super::StorageError;

pub(crate) async fn create(
    db: &DatabaseConnection,
    identity_id: &str,
    input: CreateEndpointRequest,
) -> Result<EndpointResponse, StorageError> {
    let models = normalize_models(input.models)?;
    let endpoint_id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let protocol = input.protocol.unwrap_or_else(|| "all".to_string());
    let transaction = db.begin().await?;
    ensure_identity_exists(&transaction, identity_id).await?;
    validate_models(&transaction, identity_id, &models).await?;

    identity_endpoint_configs::ActiveModel {
        id: Set(endpoint_id.clone()),
        identity_id: Set(identity_id.to_string()),
        name: Set(input.name.trim().to_string()),
        protocol: Set(protocol.trim().to_string()),
        token: Set(generate_credential()),
        created_at: Set(created_at.clone()),
    }
    .insert(&transaction)
    .await?;
    insert_models(&transaction, &endpoint_id, models, &created_at).await?;
    transaction.commit().await?;
    get(db, identity_id, &endpoint_id).await
}

pub(crate) async fn list(
    db: &DatabaseConnection,
    identity_id: &str,
) -> Result<Vec<EndpointResponse>, StorageError> {
    let endpoints = identity_endpoint_configs::Entity::find()
        .filter(identity_endpoint_configs::Column::IdentityId.eq(identity_id))
        .order_by_asc(identity_endpoint_configs::Column::CreatedAt)
        .all(db)
        .await?;
    let mut responses = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        responses.push(endpoint_response(db, endpoint).await?);
    }
    Ok(responses)
}

pub(crate) async fn get(
    db: &DatabaseConnection,
    identity_id: &str,
    endpoint_id: &str,
) -> Result<EndpointResponse, StorageError> {
    let endpoint = find_endpoint(db, identity_id, endpoint_id).await?;
    endpoint_response(db, endpoint).await
}

pub(crate) async fn update(
    db: &DatabaseConnection,
    identity_id: &str,
    endpoint_id: &str,
    input: UpdateEndpointRequest,
) -> Result<EndpointResponse, StorageError> {
    let current = find_endpoint(db, identity_id, endpoint_id).await?;
    let models = input.models.map(normalize_models).transpose()?;
    let transaction = db.begin().await?;
    if let Some(models) = &models {
        validate_models(&transaction, identity_id, models).await?;
    }

    let mut active = current.into_active_model();
    if let Some(name) = input.name {
        active.name = Set(name.trim().to_string());
    }
    if let Some(protocol) = input.protocol {
        active.protocol = Set(protocol.trim().to_string());
    }
    active.update(&transaction).await?;

    if let Some(models) = models {
        replace_models(&transaction, endpoint_id, models, &Utc::now().to_rfc3339()).await?;
    }
    transaction.commit().await?;
    get(db, identity_id, endpoint_id).await
}

pub(crate) async fn delete(
    db: &DatabaseConnection,
    identity_id: &str,
    endpoint_id: &str,
) -> Result<(), StorageError> {
    let transaction = db.begin().await?;
    find_endpoint(&transaction, identity_id, endpoint_id).await?;
    identity_endpoint_model_routes::Entity::delete_many()
        .filter(identity_endpoint_model_routes::Column::EndpointId.eq(endpoint_id))
        .exec(&transaction)
        .await?;
    identity_endpoint_models::Entity::delete_many()
        .filter(identity_endpoint_models::Column::EndpointId.eq(endpoint_id))
        .exec(&transaction)
        .await?;
    identity_endpoint_configs::Entity::delete_by_id(endpoint_id.to_string())
        .exec(&transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn regenerate_token(
    db: &DatabaseConnection,
    identity_id: &str,
    endpoint_id: &str,
) -> Result<EndpointResponse, StorageError> {
    let endpoint = find_endpoint(db, identity_id, endpoint_id).await?;
    let mut active = endpoint.into_active_model();
    active.token = Set(generate_credential());
    active.update(db).await?;
    get(db, identity_id, endpoint_id).await
}

async fn find_endpoint<C>(
    db: &C,
    identity_id: &str,
    endpoint_id: &str,
) -> Result<identity_endpoint_configs::Model, StorageError>
where
    C: ConnectionTrait,
{
    identity_endpoint_configs::Entity::find_by_id(endpoint_id)
        .filter(identity_endpoint_configs::Column::IdentityId.eq(identity_id))
        .one(db)
        .await?
        .ok_or(StorageError::EndpointNotFound)
}

async fn endpoint_response<C>(
    db: &C,
    endpoint: identity_endpoint_configs::Model,
) -> Result<EndpointResponse, StorageError>
where
    C: ConnectionTrait,
{
    let models = identity_endpoint_models::Entity::find()
        .filter(identity_endpoint_models::Column::EndpointId.eq(&endpoint.id))
        .order_by_asc(identity_endpoint_models::Column::CandidateOrder)
        .order_by_asc(identity_endpoint_models::Column::Id)
        .all(db)
        .await?
        .into_iter()
        .map(endpoint_model_response)
        .collect();
    Ok(EndpointResponse {
        id: endpoint.id,
        name: endpoint.name,
        protocol: endpoint.protocol,
        token: endpoint.token,
        models,
        created_at: endpoint.created_at,
    })
}

async fn ensure_identity_exists(
    transaction: &DatabaseTransaction,
    identity_id: &str,
) -> Result<(), StorageError> {
    if identities::Entity::find_by_id(identity_id)
        .one(transaction)
        .await?
        .is_none()
    {
        return Err(StorageError::IdentityNotFound);
    }
    Ok(())
}

#[derive(Clone)]
struct NormalizedModel {
    provider_id: String,
    upstream_model: String,
    model_name: String,
}

fn normalize_models(models: Vec<EndpointModelInput>) -> Result<Vec<NormalizedModel>, StorageError> {
    let mut mappings = HashSet::with_capacity(models.len());
    let mut normalized = Vec::with_capacity(models.len());
    for model in models {
        let upstream_model = model.upstream_model.trim().to_string();
        let model_name = model
            .model_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&upstream_model)
            .to_string();
        let mapping = (
            model_name.clone(),
            model.provider_id.clone(),
            upstream_model.clone(),
        );
        if !mappings.insert(mapping) {
            return Err(StorageError::ValidationFailed(
                "endpoint model mappings must be unique".to_string(),
            ));
        }
        normalized.push(NormalizedModel {
            provider_id: model.provider_id,
            upstream_model,
            model_name,
        });
    }
    Ok(normalized)
}

async fn validate_models(
    transaction: &DatabaseTransaction,
    identity_id: &str,
    models: &[NormalizedModel],
) -> Result<(), StorageError> {
    for model in models {
        let provider_exists = identity_provider_configs::Entity::find_by_id(&model.provider_id)
            .filter(identity_provider_configs::Column::IdentityId.eq(identity_id))
            .one(transaction)
            .await?
            .is_some();
        if !provider_exists {
            return Err(StorageError::ProviderNotFound);
        }
        let model_exists = identity_provider_models::Entity::find()
            .filter(identity_provider_models::Column::ProviderId.eq(&model.provider_id))
            .filter(identity_provider_models::Column::ModelName.eq(&model.upstream_model))
            .one(transaction)
            .await?
            .is_some();
        if !model_exists {
            return Err(StorageError::ProviderNotFound);
        }
    }
    Ok(())
}

async fn replace_models(
    transaction: &DatabaseTransaction,
    endpoint_id: &str,
    models: Vec<NormalizedModel>,
    created_at: &str,
) -> Result<(), StorageError> {
    identity_endpoint_model_routes::Entity::delete_many()
        .filter(identity_endpoint_model_routes::Column::EndpointId.eq(endpoint_id))
        .exec(transaction)
        .await?;
    identity_endpoint_models::Entity::delete_many()
        .filter(identity_endpoint_models::Column::EndpointId.eq(endpoint_id))
        .exec(transaction)
        .await?;
    insert_models(transaction, endpoint_id, models, created_at).await
}

async fn insert_models(
    transaction: &DatabaseTransaction,
    endpoint_id: &str,
    models: Vec<NormalizedModel>,
    created_at: &str,
) -> Result<(), StorageError> {
    for (candidate_order, model) in models.into_iter().enumerate() {
        identity_endpoint_models::ActiveModel {
            id: Set(Uuid::new_v4().to_string()),
            endpoint_id: Set(endpoint_id.to_string()),
            model_name: Set(model.model_name),
            provider_id: Set(model.provider_id),
            upstream_model: Set(model.upstream_model),
            candidate_order: Set(candidate_order as i64),
            created_at: Set(created_at.to_string()),
        }
        .insert(transaction)
        .await?;
    }
    Ok(())
}

fn endpoint_model_response(model: identity_endpoint_models::Model) -> EndpointModelResponse {
    EndpointModelResponse {
        id: model.id,
        endpoint_id: model.endpoint_id,
        model_name: model.model_name,
        provider_id: model.provider_id,
        upstream_model: model.upstream_model,
        created_at: model.created_at,
    }
}

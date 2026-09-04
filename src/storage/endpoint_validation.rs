use std::collections::HashSet;

use prelay_protocol::EndpointModelInput;
use sea_orm::{ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter};

use crate::{
    entity::identity::{
        provider_configs as identity_provider_configs, provider_models as identity_provider_models,
    },
    provider_catalog::ProviderCatalog,
};

use super::StorageError;

#[derive(Clone)]
pub(super) struct NormalizedModel {
    pub(super) provider_id: String,
    pub(super) upstream_model: String,
    pub(super) model_name: String,
}

pub(super) fn normalize_models(
    models: Vec<EndpointModelInput>,
) -> Result<Vec<NormalizedModel>, StorageError> {
    let mut mappings = HashSet::with_capacity(models.len());
    let mut normalized = Vec::with_capacity(models.len());
    for model in models {
        let upstream_model = model.upstream_model.trim().to_string();
        let model_name = upstream_model.clone();
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

pub(super) async fn validate_models(
    transaction: &DatabaseTransaction,
    identity_id: &str,
    models: &[NormalizedModel],
    catalog: Option<&ProviderCatalog>,
) -> Result<(), StorageError> {
    for model in models {
        if model.upstream_model.is_empty() {
            return Err(StorageError::ValidationFailed(
                "endpoint upstream model must not be empty".to_string(),
            ));
        }
        let provider = identity_provider_configs::Entity::find_by_id(&model.provider_id)
            .filter(identity_provider_configs::Column::IdentityId.eq(identity_id))
            .one(transaction)
            .await?
            .ok_or(StorageError::ProviderNotFound)?;
        let model_exists = identity_provider_models::Entity::find()
            .filter(identity_provider_models::Column::ProviderId.eq(&model.provider_id))
            .filter(identity_provider_models::Column::ModelName.eq(&model.upstream_model))
            .one(transaction)
            .await?
            .is_some();
        if !model_exists {
            return Err(StorageError::ValidationFailed(format!(
                "provider {} does not support model {}",
                model.provider_id, model.upstream_model
            )));
        }
        if let Some(catalog) = catalog {
            if !catalog
                .provider_supports_language_model(&provider.provider_type, &model.upstream_model)
                && !catalog.provider_supports_image_generation_model(
                    &provider.provider_type,
                    &model.upstream_model,
                )
            {
                return Err(StorageError::ValidationFailed(format!(
                    "provider {} does not support catalog model {}",
                    provider.provider_type, model.upstream_model
                )));
            }
        }
    }
    Ok(())
}

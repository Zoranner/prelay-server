use std::collections::HashSet;

use prelay_protocol::EndpointModelInput;
use sea_orm::{DatabaseTransaction, EntityTrait};

use crate::{
    entity::identity::provider_configs as identity_provider_configs,
    provider_catalog::ProviderCatalog,
};

use super::{provider_visibility::can_use_provider_on, StorageError};

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
        let model_name = model.upstream_model.trim().to_string();
        if model_name.is_empty() {
            return Err(StorageError::ValidationFailed(
                "endpoint upstream model must not be empty".to_string(),
            ));
        }
        if !mappings.insert((model_name.clone(), model.provider_id.clone())) {
            return Err(StorageError::ValidationFailed(
                "endpoint model mappings must be unique".to_string(),
            ));
        }
        normalized.push(NormalizedModel {
            provider_id: model.provider_id,
            upstream_model: model_name.clone(),
            model_name,
        });
    }
    Ok(normalized)
}

pub(super) async fn resolve_models(
    transaction: &DatabaseTransaction,
    identity_id: &str,
    mut models: Vec<NormalizedModel>,
    catalog: Option<&ProviderCatalog>,
) -> Result<Vec<NormalizedModel>, StorageError> {
    for model in &mut models {
        if !can_use_provider_on(transaction, identity_id, &model.provider_id).await? {
            return Err(StorageError::ProviderNotUsable);
        }
        let provider = identity_provider_configs::Entity::find_by_id(&model.provider_id)
            .one(transaction)
            .await?
            .ok_or(StorageError::ProviderNotUsable)?;        let Some(catalog) = catalog else {
            continue;
        };
        if !catalog.provider_supports_language_model(&provider.provider_type, &model.model_name)
            && !catalog.provider_supports_image_generation_model(
                &provider.provider_type,
                &model.model_name,
            )
        {
            return Err(StorageError::ValidationFailed(format!(
                "provider {} does not support catalog model {}",
                provider.provider_type, model.model_name
            )));
        }
        model.upstream_model =
            catalog.provider_upstream_model(&provider.provider_type, &model.model_name);
    }
    Ok(models)
}

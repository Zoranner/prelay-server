use std::collections::HashSet;

use crate::provider_catalog::ProviderCatalog;

use super::StorageError;

pub(super) fn normalize_model_names(models: &[String]) -> Result<Vec<String>, StorageError> {
    let mut names = HashSet::with_capacity(models.len());
    let mut normalized = Vec::with_capacity(models.len());
    for model_name in models {
        let model_name = model_name.trim();
        if model_name.is_empty() {
            return Err(StorageError::ValidationFailed(
                "provider model names must not be empty".to_string(),
            ));
        }
        if !names.insert(model_name.to_string()) {
            return Err(StorageError::ValidationFailed(
                "provider model names must be unique".to_string(),
            ));
        }
        normalized.push(model_name.to_string());
    }
    Ok(normalized)
}

pub(super) fn validate_catalog_provider(
    catalog: &ProviderCatalog,
    provider_type: &str,
    models: &[String],
) -> Result<(), StorageError> {
    let provider_type = provider_type.trim();
    let provider = catalog.provider(provider_type).ok_or_else(|| {
        StorageError::ValidationFailed(format!("unknown provider type: {provider_type}"))
    })?;
    for model in models {
        let supported = provider.language_models.iter().any(|id| id == model)
            || provider
                .image_generation_models
                .iter()
                .any(|id| id == model);
        if !supported {
            return Err(StorageError::ValidationFailed(format!(
                "model {model} is not supported by provider {provider_type}"
            )));
        }
    }
    Ok(())
}

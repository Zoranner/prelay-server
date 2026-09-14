use crate::provider_catalog::ProviderCatalog;

use super::StorageError;

pub(super) fn normalize_disabled_models(
    catalog: Option<&ProviderCatalog>,
    provider_type: &str,
    disabled_models: Vec<String>,
) -> Result<Vec<String>, StorageError> {
    let mut normalized: Vec<String> = Vec::new();
    for model_id in disabled_models {
        let model_id = model_id.trim().to_string();
        if model_id.is_empty() {
            return Err(StorageError::ValidationFailed(
                "disabled model must not be empty".to_string(),
            ));
        }
        if normalized.contains(&model_id) {
            continue;
        }
        if let Some(catalog) = catalog {
            if !catalog.provider_supports_language_model(provider_type, &model_id)
                && !catalog.provider_supports_image_generation_model(provider_type, &model_id)
            {
                return Err(StorageError::ValidationFailed(format!(
                    "provider {provider_type} does not provide model {model_id}"
                )));
            }
        }
        normalized.push(model_id);
    }
    Ok(normalized)
}

pub(super) fn disabled_models_json(disabled_models: &[String]) -> Option<String> {
    if disabled_models.is_empty() {
        return None;
    }
    serde_json::to_string(disabled_models).ok()
}

pub(super) fn parse_disabled_models(value: Option<&str>) -> Vec<String> {
    value
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}

pub(super) fn validate_catalog_provider(
    catalog: &ProviderCatalog,
    provider_type: &str,
) -> Result<(), StorageError> {
    let provider_type = provider_type.trim();
    catalog.provider(provider_type).ok_or_else(|| {
        StorageError::ValidationFailed(format!("unknown provider type: {provider_type}"))
    })?;
    Ok(())
}

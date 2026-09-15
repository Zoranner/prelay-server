use crate::provider_catalog::ProviderCatalog;

use super::StorageError;

/// 供应商启用的模型清单：去空白、去重；缺省时用目录条目的全部模型。
pub(super) fn normalize_models(models: Vec<String>) -> Result<Vec<String>, StorageError> {
    let mut normalized: Vec<String> = Vec::new();
    for model_id in models {
        let model_id = model_id.trim().to_string();
        if model_id.is_empty() {
            return Err(StorageError::ValidationFailed(
                "provider model must not be empty".to_string(),
            ));
        }
        if normalized.contains(&model_id) {
            continue;
        }
        normalized.push(model_id);
    }
    Ok(normalized)
}

/// 清单里的模型必须仍由该目录条目提供，否则要求先移除。
pub(super) fn validate_models(
    catalog: &ProviderCatalog,
    provider_type: &str,
    models: &[String],
) -> Result<(), StorageError> {
    let missing = models
        .iter()
        .filter(|model_id| {
            !catalog.provider_supports_language_model(provider_type, model_id)
                && !catalog.provider_supports_image_generation_model(provider_type, model_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(StorageError::ValidationFailed(format!(
        "供应商 {provider_type} 的目录条目里已没有模型 {}，请先移除后再保存",
        missing.join("、")
    )))
}

pub(super) fn models_json(models: &[String]) -> Option<String> {
    if models.is_empty() {
        return None;
    }
    serde_json::to_string(models).ok()
}

pub(crate) fn parse_models(value: Option<&str>) -> Vec<String> {
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

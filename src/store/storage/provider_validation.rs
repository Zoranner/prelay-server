use crate::domain::catalog::{ProviderCatalog, ProviderStatus};

use super::StorageError;

/// 供应商启用的模型清单：去空白、去重；缺省时用目录条目的全部模型。
pub(super) fn normalize_models(models: Vec<String>) -> Result<Vec<String>, StorageError> {
    let mut normalized: Vec<String> = Vec::new();
    for model_id in models {
        let model_id = model_id.trim().to_string();
        if model_id.is_empty() {
            return Err(StorageError::ValidationFailed(
                "供应商模型不能为空".to_string(),
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
    let provider = catalog.provider(provider_type).ok_or_else(|| {
        StorageError::ValidationFailed(format!("供应商类型 {provider_type} 不在当前模型目录里"))
    })?;
    match provider.status {
        ProviderStatus::Active => Ok(()),
        ProviderStatus::Deprecated => Err(StorageError::ValidationFailed(format!(
            "供应商类型 {provider_type} 已弃用，不能新建或编辑"
        ))),
        ProviderStatus::Retired => Err(StorageError::ValidationFailed(format!(
            "供应商类型 {provider_type} 已退役"
        ))),
    }
}

/// 弃用或目录外的供应商仍可服务存量请求，但不能再调整分享配置。
pub(super) fn ensure_provider_shareable(
    catalog: &ProviderCatalog,
    provider_type: &str,
) -> Result<(), StorageError> {
    match catalog.provider(provider_type) {
        Some(entry) if entry.status == ProviderStatus::Active => Ok(()),
        _ => Err(StorageError::ProviderSharingNotAllowed),
    }
}

#[cfg(test)]
mod tests {
    use super::{ensure_provider_shareable, validate_catalog_provider};
    use crate::store::storage::StorageError;

    #[test]
    fn rejects_deprecated_provider_types() {
        let catalog = crate::test_support::fixture_catalog();

        let error = validate_catalog_provider(&catalog, "legacy_relay")
            .expect_err("deprecated provider type must be rejected");

        assert!(matches!(
            error,
            StorageError::ValidationFailed(message) if message.contains("已弃用")
        ));
    }

    #[test]
    fn rejects_provider_types_missing_from_the_catalog() {
        let catalog = crate::test_support::fixture_catalog();

        let error = validate_catalog_provider(&catalog, "retired_provider")
            .expect_err("provider type outside the catalog must be rejected");

        assert!(matches!(
            error,
            StorageError::ValidationFailed(message) if message.contains("不在当前模型目录里")
        ));
    }

    #[test]
    fn rejects_retired_provider_types() {
        let catalog = crate::test_support::fixture_catalog();

        let error = validate_catalog_provider(&catalog, "retired_relay")
            .expect_err("retired provider type must be rejected");

        assert!(matches!(
            error,
            StorageError::ValidationFailed(message) if message.contains("已退役")
        ));
    }

    #[test]
    fn accepts_active_catalog_provider_types() {
        let catalog = crate::test_support::fixture_catalog();

        validate_catalog_provider(&catalog, "deepseek").expect("active provider type");
    }

    #[test]
    fn sharing_requires_an_active_provider_type() {
        let catalog = crate::test_support::fixture_catalog();

        ensure_provider_shareable(&catalog, "deepseek").expect("active provider is shareable");
        for provider_type in ["legacy_relay", "retired_relay", "missing_provider"] {
            assert!(matches!(
                ensure_provider_shareable(&catalog, provider_type),
                Err(StorageError::ProviderSharingNotAllowed)
            ));
        }
    }
}

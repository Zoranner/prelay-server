use crate::provider_catalog::ProviderCatalog;

use super::StorageError;

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

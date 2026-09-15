use prelay_protocol::{
    ProviderCapabilityOverrides, ProviderListItemResponse, ProviderResponse, ProviderVisibility,
};

use crate::{
    entity::identity::provider_configs as identity_provider_configs,
    provider_catalog::ProviderCatalog, providers::spec::resolved_upstream_protocols,
};

use super::{crypto::KeyCipher, StorageError};

pub(super) fn provider_response(
    crypto: &KeyCipher,
    catalog: Option<&ProviderCatalog>,
    provider: identity_provider_configs::Model,
) -> Result<ProviderResponse, StorageError> {
    let capabilities = capabilities(&provider);
    let upstream_protocols = resolved_upstream_protocols(catalog, &provider.provider_type);
    let models = models(&provider);
    let api_key = crypto.decrypt(&provider.api_key_ciphertext)?;
    Ok(ProviderResponse {
        id: provider.id,
        name: provider.name,
        provider_type: provider.provider_type,
        base_url: provider.base_url,
        api_key,
        api_key_masked: mask_ciphertext(&provider.api_key_ciphertext),
        capabilities,
        upstream_protocols,
        models,
        created_at: provider.created_at,
    })
}

pub(super) fn provider_list_item(
    catalog: Option<&ProviderCatalog>,
    provider: identity_provider_configs::Model,
    owner_identity_id: String,
    owner_display_name: String,
    visibility: ProviderVisibility,
    selected_identity_ids: Vec<String>,
    can_manage: bool,
) -> Result<ProviderListItemResponse, StorageError> {
    let capabilities = capabilities(&provider);
    let upstream_protocols = resolved_upstream_protocols(catalog, &provider.provider_type);
    let models = models(&provider);
    Ok(ProviderListItemResponse {
        id: provider.id,
        name: provider.name,
        provider_type: provider.provider_type,
        base_url: provider.base_url,
        capabilities,
        upstream_protocols,
        models,
        owner_identity_id,
        owner_display_name,
        visibility,
        selected_identity_ids,
        can_manage,
        created_at: provider.created_at,
    })
}

fn capabilities(provider: &identity_provider_configs::Model) -> ProviderCapabilityOverrides {
    let mut capabilities: ProviderCapabilityOverrides = provider
        .capabilities_json
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default();
    // 协议集合由目录定义，历史记录里的协议集合不再对外返回。
    capabilities.upstream_protocols = None;
    capabilities
}

fn models(provider: &identity_provider_configs::Model) -> Vec<String> {
    super::provider_validation::parse_models(provider.models_json.as_deref())
}

fn mask_ciphertext(ciphertext: &str) -> String {
    if ciphertext.is_empty() {
        String::new()
    } else {
        "********".to_string()
    }
}

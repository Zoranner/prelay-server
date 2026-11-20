use prelay_protocol::{ProviderCapabilityOverrides, ProviderCredential, ProviderResponse};

use crate::{
    codec::providers::spec::resolved_upstream_protocols, domain::catalog::ProviderCatalog,
    store::entity::identity::provider_configs as identity_provider_configs,
};

use super::{crypto::KeyCipher, provider_visibility::VisibleProvider, StorageError};

/// Provider 唯一读模型：列表、编辑前加载和保存回写都从这里投影。
/// `credential` 只在创建人读取编辑数据时填充，列表恒为 `None`。
pub(super) fn provider_response(
    crypto: &KeyCipher,
    catalog: Option<&ProviderCatalog>,
    visible: VisibleProvider,
    include_credential: bool,
) -> Result<ProviderResponse, StorageError> {
    let VisibleProvider {
        provider,
        owner_identity_id,
        owner_display_name,
        visibility,
        ..
    } = visible;
    let capabilities = capabilities(&provider);
    let upstream_protocols = resolved_upstream_protocols(catalog, &provider.provider_type);
    let models = models(&provider);
    let credential = if include_credential {
        Some(ProviderCredential {
            api_key: crypto.decrypt(&provider.api_key_ciphertext)?,
            api_key_masked: mask_ciphertext(&provider.api_key_ciphertext),
        })
    } else {
        None
    };
    Ok(ProviderResponse {
        id: provider.id,
        name: provider.name,
        provider_type: provider.provider_type,
        base_url: provider.base_url,
        capabilities,
        upstream_protocols,
        models,
        created_at: provider.created_at,
        owner_identity_id,
        owner_display_name,
        visibility,
        credential,
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

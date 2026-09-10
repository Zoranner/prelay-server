use prelay_protocol::{
    ProviderCapabilityOverrides, ProviderListItemResponse, ProviderResponse, ProviderVisibility,
};

use crate::{
    entity::identity::provider_configs as identity_provider_configs,
    providers::spec::resolved_upstream_protocols,
};

use super::{crypto::KeyCipher, StorageError};

pub(super) fn provider_response(
    crypto: &KeyCipher,
    provider: identity_provider_configs::Model,
) -> Result<ProviderResponse, StorageError> {
    let capabilities: ProviderCapabilityOverrides = provider
        .capabilities_json
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default();
    let upstream_protocols = resolved_upstream_protocols(
        &provider.provider_type,
        capabilities.upstream_protocols.as_deref(),
    );
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
        created_at: provider.created_at,
    })
}

pub(super) fn provider_list_item(
    provider: identity_provider_configs::Model,
    owner_identity_id: String,
    owner_display_name: String,
    visibility: ProviderVisibility,
    selected_identity_ids: Vec<String>,
    can_manage: bool,
) -> Result<ProviderListItemResponse, StorageError> {
    let capabilities = capabilities(&provider);
    let upstream_protocols = resolved_upstream_protocols(
        &provider.provider_type,
        capabilities.upstream_protocols.as_deref(),
    );
    Ok(ProviderListItemResponse {
        id: provider.id,
        name: provider.name,
        provider_type: provider.provider_type,
        base_url: provider.base_url,
        capabilities,
        upstream_protocols,
        owner_identity_id,
        owner_display_name,
        visibility,
        selected_identity_ids,
        can_manage,
        created_at: provider.created_at,
    })
}

fn capabilities(provider: &identity_provider_configs::Model) -> ProviderCapabilityOverrides {
    provider
        .capabilities_json
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}

fn mask_ciphertext(ciphertext: &str) -> String {
    if ciphertext.is_empty() {
        String::new()
    } else {
        "********".to_string()
    }
}

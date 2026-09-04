use prelay_protocol::{ProviderCapabilityOverrides, ProviderModelResponse, ProviderResponse};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::{
    entity::identity::{
        provider_configs as identity_provider_configs, provider_models as identity_provider_models,
    },
    provider_catalog::ProviderCatalog,
    providers::spec::resolved_upstream_protocols,
};

use super::{crypto::KeyCipher, StorageError};

pub(super) async fn provider_response<C>(
    db: &C,
    crypto: &KeyCipher,
    provider: identity_provider_configs::Model,
    catalog: Option<&ProviderCatalog>,
) -> Result<ProviderResponse, StorageError>
where
    C: ConnectionTrait,
{
    let models = identity_provider_models::Entity::find()
        .filter(identity_provider_models::Column::ProviderId.eq(&provider.id))
        .order_by_asc(identity_provider_models::Column::CreatedAt)
        .all(db)
        .await?
        .into_iter()
        .map(|model| provider_model_response(model, catalog))
        .collect();
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
        models,
        created_at: provider.created_at,
    })
}

fn provider_model_response(
    model: identity_provider_models::Model,
    catalog: Option<&ProviderCatalog>,
) -> ProviderModelResponse {
    let display_name = catalog
        .map(|catalog| catalog.model_display_name(&model.model_name))
        .unwrap_or_else(|| model.model_name.clone());
    ProviderModelResponse {
        id: model.id,
        provider_id: model.provider_id,
        model_name: model.model_name,
        display_name,
        created_at: model.created_at,
    }
}

fn mask_ciphertext(ciphertext: &str) -> String {
    if ciphertext.is_empty() {
        String::new()
    } else {
        "********".to_string()
    }
}

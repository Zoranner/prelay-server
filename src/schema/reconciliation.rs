use std::collections::HashMap;

use crate::{
    entity::identity::{endpoint_model_routes, endpoint_models, provider_configs},
    models::ProviderCapabilityOverrides,
    provider_catalog::ProviderCatalog,
};
use prelay_protocol::ProviderProtocol;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    IntoActiveModel, QueryFilter,
};

/// 供应商协议地址以目录为准：目录声明的协议键一律对齐到记录（覆盖旧值），
/// 目录未声明的键保留，目录条目缺失时保留记录不动。启动时执行，幂等。
pub(crate) async fn reconcile_protocol_base_urls(
    db: &DatabaseConnection,
    catalog: &ProviderCatalog,
) -> Result<(), DbErr> {
    for provider in provider_configs::Entity::find().all(db).await? {
        let Some(catalog_provider) = catalog.provider(&provider.provider_type) else {
            continue;
        };
        if catalog_provider.protocol_base_urls.is_empty() {
            continue;
        }
        let mut capabilities: ProviderCapabilityOverrides = provider
            .capabilities_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default();
        let mut base_urls = capabilities.protocol_base_urls.take().unwrap_or_default();
        let mut changed = false;
        for (protocol, url) in &catalog_provider.protocol_base_urls {
            let slot = match protocol {
                ProviderProtocol::ChatCompletions => &mut base_urls.openai,
                ProviderProtocol::Responses => &mut base_urls.responses,
                ProviderProtocol::ImagesGenerations => &mut base_urls.images_generations,
            };
            if slot.as_deref() != Some(url.as_str()) {
                *slot = Some(url.clone());
                changed = true;
            }
        }
        if !changed {
            continue;
        }
        capabilities.protocol_base_urls = Some(base_urls);
        let updated = serde_json::to_string(&capabilities)
            .map_err(|error| DbErr::Custom(format!("serialize capabilities: {error}")))?;
        let mut active = provider.into_active_model();
        active.capabilities_json = Set(Some(updated));
        active.update(db).await?;
    }
    Ok(())
}

/// 接入点模型始终按当前目录推导：供应商缺失或模型不再由该供应商提供时删除映射，
/// 供应商上游名变化时回填。启动时执行，幂等。
pub(crate) async fn reconcile_endpoint_models(
    db: &DatabaseConnection,
    catalog: &ProviderCatalog,
) -> Result<(), DbErr> {
    let providers = provider_configs::Entity::find().all(db).await?;
    let provider_types = providers
        .into_iter()
        .map(|provider| {
            (
                provider.id,
                (
                    provider.provider_type,
                    crate::storage::parse_models(provider.models_json.as_deref()),
                ),
            )
        })
        .collect::<HashMap<_, _>>();

    for model in endpoint_models::Entity::find().all(db).await? {
        let Some((provider_type, provider_models)) = provider_types.get(&model.provider_id) else {
            tracing::warn!(
                endpoint_model = %model.id,
                provider = %model.provider_id,
                "removing endpoint model whose provider no longer exists"
            );
            delete_endpoint_model(db, &model).await?;
            continue;
        };
        if !provider_models.contains(&model.model_name) {
            tracing::warn!(
                endpoint_model = %model.id,
                provider = %model.provider_id,
                model = %model.model_name,
                "removing endpoint model that the provider no longer enables"
            );
            delete_endpoint_model(db, &model).await?;
            continue;
        }
        if catalog.provider(provider_type).is_none() {
            continue;
        }
        let upstream_model = catalog.provider_upstream_model(provider_type, &model.model_name);
        if model.upstream_model != upstream_model {
            let mut active = model.into_active_model();
            active.upstream_model = Set(upstream_model);
            active.update(db).await?;
        }
    }
    Ok(())
}

async fn delete_endpoint_model(
    db: &DatabaseConnection,
    model: &endpoint_models::Model,
) -> Result<(), DbErr> {
    endpoint_model_routes::Entity::delete_many()
        .filter(endpoint_model_routes::Column::EndpointId.eq(model.endpoint_id.clone()))
        .filter(endpoint_model_routes::Column::ModelName.eq(model.model_name.clone()))
        .filter(endpoint_model_routes::Column::ProviderId.eq(model.provider_id.clone()))
        .exec(db)
        .await?;
    endpoint_models::Entity::delete_by_id(model.id.clone())
        .exec(db)
        .await?;
    Ok(())
}

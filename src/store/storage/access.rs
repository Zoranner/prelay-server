use std::time::Instant;

use chrono::Utc;
use sea_orm::{
    sea_query::OnConflict, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use super::{identities, ProtocolAccess, ProtocolModel, Storage, StorageError};
use crate::store::entity::identity::{
    endpoint_configs, endpoint_model_routes, endpoint_models, provider_configs,
};
use crate::store::recording_timing::log_elapsed;

use super::provider_visibility::can_use_provider_on;

impl Storage {
    pub async fn authenticate_protocol_access(
        &self,
        token: &str,
    ) -> Result<Option<ProtocolAccess>, StorageError> {
        let endpoint = endpoint_configs::Entity::find()
            .filter(endpoint_configs::Column::Token.eq(token))
            .one(&self.db)
            .await?;
        let access = endpoint.map(|endpoint| ProtocolAccess {
            identity_id: endpoint.identity_id,
            endpoint_id: endpoint.id,
            endpoint_name: endpoint.name,
        });
        if let Some(access) = &access {
            identities::touch(&self.db, &access.identity_id).await?;
        }
        Ok(access)
    }

    pub async fn resolve_protocol_models(
        &self,
        access: &ProtocolAccess,
        model_name: &str,
    ) -> Result<Vec<ProtocolModel>, StorageError> {
        self.load_protocol_models(access, Some(model_name)).await
    }

    /// 记录最近一次成功响应的供应商，仅作为观测数据保存。
    ///
    /// 选路不再读取该记录；会话粘性由进程内的 `SessionRouteRegistry` 维护。
    pub async fn remember_protocol_model_provider(
        &self,
        access: &ProtocolAccess,
        model_name: &str,
        provider_id: &str,
    ) -> Result<(), StorageError> {
        let observed_at = Utc::now().to_rfc3339();
        self.remember_protocol_model_provider_at(access, model_name, provider_id, &observed_at)
            .await
    }

    pub(crate) async fn remember_protocol_model_provider_at(
        &self,
        access: &ProtocolAccess,
        model_name: &str,
        provider_id: &str,
        observed_at: &str,
    ) -> Result<(), StorageError> {
        let started_at = Instant::now();
        let route = endpoint_model_routes::ActiveModel {
            endpoint_id: Set(access.endpoint_id.clone()),
            model_name: Set(model_name.to_string()),
            provider_id: Set(provider_id.to_string()),
            updated_at: Set(observed_at.to_string()),
        };
        endpoint_model_routes::Entity::insert(route)
            .on_conflict(
                OnConflict::columns([
                    endpoint_model_routes::Column::EndpointId,
                    endpoint_model_routes::Column::ModelName,
                ])
                .update_columns([
                    endpoint_model_routes::Column::ProviderId,
                    endpoint_model_routes::Column::UpdatedAt,
                ])
                .to_owned(),
            )
            .exec(&self.db)
            .await?;
        log_elapsed(
            "storage.remember_protocol_model_provider",
            started_at,
            Some(model_name),
        );
        Ok(())
    }

    pub async fn list_protocol_models(
        &self,
        access: &ProtocolAccess,
    ) -> Result<Vec<ProtocolModel>, StorageError> {
        self.load_protocol_models(access, None).await
    }

    async fn load_protocol_models(
        &self,
        access: &ProtocolAccess,
        model_name: Option<&str>,
    ) -> Result<Vec<ProtocolModel>, StorageError> {
        let endpoint_exists = endpoint_configs::Entity::find_by_id(&access.endpoint_id)
            .filter(endpoint_configs::Column::IdentityId.eq(&access.identity_id))
            .one(&self.db)
            .await?
            .is_some();
        if !endpoint_exists {
            return Ok(Vec::new());
        }

        let mut query = endpoint_models::Entity::find()
            .filter(endpoint_models::Column::EndpointId.eq(&access.endpoint_id));
        if let Some(model_name) = model_name {
            query = query.filter(endpoint_models::Column::ModelName.eq(model_name));
        }
        let models = query
            .order_by_asc(endpoint_models::Column::CandidateOrder)
            .order_by_asc(endpoint_models::Column::Id)
            .all(&self.db)
            .await?;
        let mut resolved = Vec::with_capacity(models.len());
        for model in models {
            let provider = provider_configs::Entity::find_by_id(&model.provider_id)
                .one(&self.db)
                .await?;
            let Some(provider) = provider else {
                continue;
            };
            if !can_use_provider_on(&self.db, &access.identity_id, &model.provider_id).await? {
                continue;
            }
            resolved.push(ProtocolModel {
                model: crate::domain::registry::EndpointModel {
                    id: model.id,
                    endpoint_id: model.endpoint_id,
                    model_name: model.model_name,
                    provider_id: model.provider_id,
                    upstream_model: model.upstream_model,
                    created_at: model.created_at,
                },
                provider: crate::domain::registry::ProviderConfig {
                    id: provider.id,
                    name: provider.name,
                    provider_type: provider.provider_type,
                    base_url: provider.base_url,
                    api_key: self.crypto.decrypt(&provider.api_key_ciphertext)?,
                    capabilities_json: provider.capabilities_json,
                    models: super::provider_validation::parse_models(
                        provider.models_json.as_deref(),
                    ),
                    created_at: provider.created_at,
                },
            });
        }
        Ok(resolved)
    }
}

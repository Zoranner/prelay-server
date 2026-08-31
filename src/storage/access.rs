use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sea_orm::{
    sea_query::OnConflict, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::entity::identity::{
    activities, endpoint_configs, endpoint_model_routes, endpoint_models, provider_configs,
    provider_models,
};

use super::{identities, ProtocolAccess, ProtocolModel, Storage, StorageError};

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

    pub async fn select_protocol_model_candidates(
        &self,
        access: &ProtocolAccess,
        model_name: &str,
    ) -> Result<Vec<ProtocolModel>, StorageError> {
        let mut candidates = self.resolve_protocol_models(access, model_name).await?;
        if candidates.len() < 2 {
            return Ok(candidates);
        }

        let provider_latencies = self
            .provider_average_latencies(
                &access.identity_id,
                candidates
                    .iter()
                    .map(|candidate| candidate.provider.id.as_str()),
            )
            .await?;
        let sort_by_latency = |left: &ProtocolModel, right: &ProtocolModel| match (
            provider_latencies.get(&left.provider.id),
            provider_latencies.get(&right.provider.id),
        ) {
            (Some(left), Some(right)) => {
                left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };

        let active_provider_id = endpoint_model_routes::Entity::find_by_id((
            access.endpoint_id.clone(),
            model_name.to_string(),
        ))
        .one(&self.db)
        .await?
        .map(|route| route.provider_id);
        if let Some(active_provider_id) = active_provider_id {
            if let Some(index) = candidates
                .iter()
                .position(|candidate| candidate.provider.id == active_provider_id)
            {
                let active = candidates.remove(index);
                candidates.sort_by(sort_by_latency);
                candidates.insert(0, active);
                return Ok(candidates);
            }
        }

        candidates.sort_by(sort_by_latency);
        Ok(candidates)
    }

    async fn provider_average_latencies<'a>(
        &self,
        identity_id: &str,
        provider_ids: impl Iterator<Item = &'a str>,
    ) -> Result<HashMap<String, f64>, StorageError> {
        let provider_ids = provider_ids
            .map(str::to_string)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if provider_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = activities::Entity::find()
            .filter(activities::Column::IdentityId.eq(identity_id))
            .filter(activities::Column::Status.eq("success"))
            .filter(activities::Column::ProviderId.is_in(provider_ids))
            .filter(activities::Column::UpstreamLatencyMs.is_not_null())
            .all(&self.db)
            .await?;
        let mut totals = HashMap::<String, (i128, u64)>::new();
        for row in rows {
            let (Some(provider_id), Some(latency)) = (row.provider_id, row.upstream_latency_ms)
            else {
                continue;
            };
            let total = totals.entry(provider_id).or_default();
            total.0 += i128::from(latency);
            total.1 += 1;
        }
        Ok(totals
            .into_iter()
            .map(|(provider_id, (sum, count))| (provider_id, sum as f64 / count as f64))
            .collect())
    }

    pub async fn remember_protocol_model_provider(
        &self,
        access: &ProtocolAccess,
        model_name: &str,
        provider_id: &str,
    ) -> Result<(), StorageError> {
        let route = endpoint_model_routes::ActiveModel {
            endpoint_id: Set(access.endpoint_id.clone()),
            model_name: Set(model_name.to_string()),
            provider_id: Set(provider_id.to_string()),
            updated_at: Set(Utc::now().to_rfc3339()),
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
                .filter(provider_configs::Column::IdentityId.eq(&access.identity_id))
                .one(&self.db)
                .await?;
            let Some(provider) = provider else {
                continue;
            };
            let upstream_exists = provider_models::Entity::find()
                .filter(provider_models::Column::ProviderId.eq(&provider.id))
                .filter(provider_models::Column::ModelName.eq(&model.upstream_model))
                .one(&self.db)
                .await?
                .is_some();
            if !upstream_exists {
                continue;
            }
            resolved.push(ProtocolModel {
                model: crate::models::EndpointModel {
                    id: model.id,
                    endpoint_id: model.endpoint_id,
                    model_name: model.model_name,
                    provider_id: model.provider_id,
                    upstream_model: model.upstream_model,
                    created_at: model.created_at,
                },
                provider: crate::models::ProviderConfig {
                    id: provider.id,
                    name: provider.name,
                    provider_type: provider.provider_type,
                    base_url: provider.base_url,
                    api_key: self.crypto.decrypt(&provider.api_key_ciphertext)?,
                    token: String::new(),
                    capabilities_json: provider.capabilities_json,
                    created_at: provider.created_at,
                },
            });
        }
        Ok(resolved)
    }
}

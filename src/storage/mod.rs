mod crypto;
mod identities;
mod sessions;
mod stats;

pub mod endpoints;
pub mod providers;

use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use chrono::{DateTime, Duration, Utc};
use prelay_protocol::{
    CreateEndpointRequest, CreateIdentityResponse, CreateProviderRequest, EndpointResponse,
    ProtocolErrorCode, ProviderResponse, RotateCredentialResponse,
};
use sea_orm::{
    sea_query::OnConflict, ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder,
};

use crate::entity::{
    identity_endpoint_configs, identity_endpoint_model_routes, identity_endpoint_models,
    identity_provider_configs, identity_provider_models, identity_request_logs,
};
use crate::stats::{
    ModelStatsSummary, ProviderStatsSummary, RequestLogInsert, RequestLogSummary, StatsOverview,
    StatsRange, StreamRequestLogUpdate, TokenUsageTimelinePoint,
};

pub use crypto::MasterKey;
pub use identities::AuthenticatedIdentity;
pub use sessions::ResponseSessionInsert;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolAccess {
    pub identity_id: String,
    pub endpoint_id: String,
    pub endpoint_name: String,
}

#[derive(Clone, Debug)]
pub struct ProtocolModel {
    pub model: crate::models::EndpointModel,
    pub provider: crate::models::ProviderConfig,
}

#[derive(Clone)]
pub struct Storage {
    db: DatabaseConnection,
    crypto: crypto::KeyCipher,
}

#[derive(Debug)]
pub enum StorageError {
    IdentityAlreadyRegistered,
    IdentityNotFound,
    InvalidCredential,
    ProviderNotFound,
    EndpointNotFound,
    RequestLogNotFound,
    ValidationFailed(String),
    InvalidTimestamp(String),
    InvalidMasterKey(String),
    Crypto(String),
    Serialization(serde_json::Error),
    Database(DbErr),
}

impl StorageError {
    pub const fn code(&self) -> ProtocolErrorCode {
        match self {
            Self::IdentityAlreadyRegistered => ProtocolErrorCode::IdentityAlreadyRegistered,
            Self::InvalidCredential => ProtocolErrorCode::InvalidCredential,
            Self::IdentityNotFound
            | Self::ProviderNotFound
            | Self::EndpointNotFound
            | Self::RequestLogNotFound => ProtocolErrorCode::NotFound,
            Self::InvalidMasterKey(_) | Self::ValidationFailed(_) => {
                ProtocolErrorCode::ValidationFailed
            }
            Self::InvalidTimestamp(_)
            | Self::Crypto(_)
            | Self::Serialization(_)
            | Self::Database(_) => ProtocolErrorCode::Internal,
        }
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityAlreadyRegistered => {
                formatter.write_str("identity is already registered")
            }
            Self::IdentityNotFound => formatter.write_str("identity does not exist"),
            Self::InvalidCredential => formatter.write_str("device credential is no longer valid"),
            Self::ProviderNotFound => formatter.write_str("provider does not exist for identity"),
            Self::EndpointNotFound => formatter.write_str("endpoint does not exist for identity"),
            Self::RequestLogNotFound => {
                formatter.write_str("request log does not exist for identity")
            }
            Self::ValidationFailed(message) => formatter.write_str(message),
            Self::InvalidTimestamp(message) => {
                write!(formatter, "invalid stored timestamp: {message}")
            }
            Self::InvalidMasterKey(message) => write!(formatter, "invalid master key: {message}"),
            Self::Crypto(message) => write!(formatter, "key encryption failed: {message}"),
            Self::Serialization(error) => error.fmt(formatter),
            Self::Database(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialization(error) => Some(error),
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DbErr> for StorageError {
    fn from(error: DbErr) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

impl Storage {
    pub fn from_connection(db: DatabaseConnection, master_key: MasterKey) -> Self {
        Self {
            db,
            crypto: crypto::KeyCipher::new(master_key),
        }
    }

    pub async fn save_response_session(
        &self,
        insert: ResponseSessionInsert<'_>,
    ) -> Result<(), StorageError> {
        sessions::save_response_session(&self.db, insert).await
    }

    pub async fn load_response_session_messages(
        &self,
        identity_id: &str,
        response_id: &str,
    ) -> Result<Option<Vec<crate::bridge::internal::InternalMessage>>, StorageError> {
        sessions::load_response_session_messages(&self.db, identity_id, response_id).await
    }

    pub async fn insert_request_log(
        &self,
        identity_id: &str,
        log: RequestLogInsert,
    ) -> Result<(), StorageError> {
        stats::insert(&self.db, identity_id, log).await
    }

    pub async fn insert_request_log_with_id(
        &self,
        identity_id: &str,
        id: String,
        log: RequestLogInsert,
    ) -> Result<(), StorageError> {
        stats::insert_with_id(&self.db, identity_id, id, log).await
    }

    pub async fn update_stream_request_log(
        &self,
        identity_id: &str,
        id: &str,
        update: StreamRequestLogUpdate,
    ) -> Result<(), StorageError> {
        stats::update_stream(&self.db, identity_id, id, update).await
    }

    pub async fn stats_overview(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<StatsOverview, StorageError> {
        stats::overview(&self.db, identity_id, range).await
    }

    pub async fn list_request_logs(
        &self,
        identity_id: &str,
        limit: usize,
    ) -> Result<Vec<RequestLogSummary>, StorageError> {
        stats::list_requests(&self.db, identity_id, limit).await
    }

    pub async fn model_stats(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<ModelStatsSummary>, StorageError> {
        stats::list_model_stats(&self.db, identity_id, range).await
    }

    pub async fn provider_stats(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<ProviderStatsSummary>, StorageError> {
        stats::list_provider_stats(&self.db, identity_id, range).await
    }

    pub async fn token_usage_timeline(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
        stats::timeline(&self.db, identity_id, range).await
    }

    pub async fn register_identity(
        &self,
        machine_id: &str,
        account_sid: &str,
        credential: &str,
    ) -> Result<CreateIdentityResponse, StorageError> {
        self.register_identity_with_display_name(machine_id, account_sid, credential, None)
            .await
    }

    pub async fn register_identity_with_display_name(
        &self,
        machine_id: &str,
        account_sid: &str,
        credential: &str,
        display_name: Option<&str>,
    ) -> Result<CreateIdentityResponse, StorageError> {
        identities::register(&self.db, machine_id, account_sid, credential, display_name).await
    }

    pub async fn authenticate_identity(
        &self,
        credential: &str,
    ) -> Result<Option<AuthenticatedIdentity>, StorageError> {
        self.authenticate_identity_with_display_name(credential, None)
            .await
    }

    pub async fn authenticate_identity_with_display_name(
        &self,
        credential: &str,
        display_name: Option<&str>,
    ) -> Result<Option<AuthenticatedIdentity>, StorageError> {
        identities::authenticate(&self.db, credential, display_name).await
    }

    pub async fn rotate_identity_credential(
        &self,
        identity_id: &str,
        authenticated_credential_hash: &str,
        new_credential: &str,
    ) -> Result<RotateCredentialResponse, StorageError> {
        identities::rotate_credential(
            &self.db,
            identity_id,
            authenticated_credential_hash,
            new_credential,
        )
        .await
    }

    pub async fn identity_credential_hash(
        &self,
        identity_id: &str,
    ) -> Result<String, StorageError> {
        identities::credential_hash(&self.db, identity_id).await
    }

    pub async fn delete_inactive_identities(
        &self,
        now: DateTime<Utc>,
        retention: Duration,
    ) -> Result<u64, StorageError> {
        identities::delete_inactive(&self.db, now, retention).await
    }

    pub async fn create_provider(
        &self,
        identity_id: &str,
        input: CreateProviderRequest,
    ) -> Result<String, StorageError> {
        providers::create(&self.db, &self.crypto, identity_id, input).await
    }

    pub async fn raw_provider_key_ciphertext(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<String, StorageError> {
        providers::raw_key_ciphertext(&self.db, identity_id, provider_id).await
    }

    pub async fn decrypt_provider_key(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<String, StorageError> {
        let ciphertext = self
            .raw_provider_key_ciphertext(identity_id, provider_id)
            .await?;
        self.crypto.decrypt(&ciphertext)
    }

    pub async fn list_providers(
        &self,
        identity_id: &str,
    ) -> Result<Vec<ProviderResponse>, StorageError> {
        providers::list(&self.db, &self.crypto, identity_id).await
    }

    pub async fn get_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<ProviderResponse, StorageError> {
        providers::get(&self.db, &self.crypto, identity_id, provider_id).await
    }

    pub async fn update_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
        input: prelay_protocol::providers::UpdateProviderRequest,
    ) -> Result<ProviderResponse, StorageError> {
        providers::update(&self.db, &self.crypto, identity_id, provider_id, input).await
    }

    pub async fn delete_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<(), StorageError> {
        providers::delete(&self.db, identity_id, provider_id).await
    }

    pub async fn add_provider_models(
        &self,
        identity_id: &str,
        provider_id: &str,
        model_names: &[String],
    ) -> Result<(), StorageError> {
        providers::add_models(&self.db, identity_id, provider_id, model_names).await
    }

    pub async fn create_interface(
        &self,
        identity_id: &str,
        input: CreateEndpointRequest,
    ) -> Result<EndpointResponse, StorageError> {
        endpoints::create(&self.db, identity_id, input).await
    }

    pub async fn list_endpoints(
        &self,
        identity_id: &str,
    ) -> Result<Vec<EndpointResponse>, StorageError> {
        endpoints::list(&self.db, identity_id).await
    }

    pub async fn get_interface(
        &self,
        identity_id: &str,
        endpoint_id: &str,
    ) -> Result<EndpointResponse, StorageError> {
        endpoints::get(&self.db, identity_id, endpoint_id).await
    }

    pub async fn update_interface(
        &self,
        identity_id: &str,
        endpoint_id: &str,
        input: prelay_protocol::endpoints::UpdateEndpointRequest,
    ) -> Result<EndpointResponse, StorageError> {
        endpoints::update(&self.db, identity_id, endpoint_id, input).await
    }

    pub async fn delete_interface(
        &self,
        identity_id: &str,
        endpoint_id: &str,
    ) -> Result<(), StorageError> {
        endpoints::delete(&self.db, identity_id, endpoint_id).await
    }

    pub async fn regenerate_endpoint_token(
        &self,
        identity_id: &str,
        endpoint_id: &str,
    ) -> Result<EndpointResponse, StorageError> {
        endpoints::regenerate_token(&self.db, identity_id, endpoint_id).await
    }

    pub async fn authenticate_protocol_access(
        &self,
        token: &str,
    ) -> Result<Option<ProtocolAccess>, StorageError> {
        let endpoint = identity_endpoint_configs::Entity::find()
            .filter(identity_endpoint_configs::Column::Token.eq(token))
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

        let active_provider_id = identity_endpoint_model_routes::Entity::find_by_id((
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
        let rows = identity_request_logs::Entity::find()
            .filter(identity_request_logs::Column::IdentityId.eq(identity_id))
            .filter(identity_request_logs::Column::Status.eq("success"))
            .filter(identity_request_logs::Column::ProviderId.is_in(provider_ids))
            .filter(identity_request_logs::Column::UpstreamLatencyMs.is_not_null())
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
        let route = identity_endpoint_model_routes::ActiveModel {
            endpoint_id: Set(access.endpoint_id.clone()),
            model_name: Set(model_name.to_string()),
            provider_id: Set(provider_id.to_string()),
            updated_at: Set(Utc::now().to_rfc3339()),
        };
        identity_endpoint_model_routes::Entity::insert(route)
            .on_conflict(
                OnConflict::columns([
                    identity_endpoint_model_routes::Column::EndpointId,
                    identity_endpoint_model_routes::Column::ModelName,
                ])
                .update_columns([
                    identity_endpoint_model_routes::Column::ProviderId,
                    identity_endpoint_model_routes::Column::UpdatedAt,
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
        let endpoint_exists = identity_endpoint_configs::Entity::find_by_id(&access.endpoint_id)
            .filter(identity_endpoint_configs::Column::IdentityId.eq(&access.identity_id))
            .one(&self.db)
            .await?
            .is_some();
        if !endpoint_exists {
            return Ok(Vec::new());
        }

        let mut query = identity_endpoint_models::Entity::find()
            .filter(identity_endpoint_models::Column::EndpointId.eq(&access.endpoint_id));
        if let Some(model_name) = model_name {
            query = query.filter(identity_endpoint_models::Column::ModelName.eq(model_name));
        }
        let models = query
            .order_by_asc(identity_endpoint_models::Column::CandidateOrder)
            .order_by_asc(identity_endpoint_models::Column::Id)
            .all(&self.db)
            .await?;
        let mut resolved = Vec::with_capacity(models.len());
        for model in models {
            let provider = identity_provider_configs::Entity::find_by_id(&model.provider_id)
                .filter(identity_provider_configs::Column::IdentityId.eq(&access.identity_id))
                .one(&self.db)
                .await?;
            let Some(provider) = provider else {
                continue;
            };
            let upstream_exists = identity_provider_models::Entity::find()
                .filter(identity_provider_models::Column::ProviderId.eq(&provider.id))
                .filter(identity_provider_models::Column::ModelName.eq(&model.upstream_model))
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

mod crypto;
mod identities;
mod schema;

pub mod endpoints;
pub mod providers;
pub mod sessions;
pub mod stats;

use std::{collections::HashMap, fmt};

use chrono::{DateTime, Duration, Utc};

use prelay_protocol::{
    CreateEndpointRequest, CreateIdentityResponse, CreateProviderRequest, EndpointResponse,
    ProtocolErrorCode, ProviderResponse, RotateCredentialResponse,
};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    QueryBuilder, Sqlite, SqlitePool,
};
use std::str::FromStr;

pub use crypto::MasterKey;
pub use identities::AuthenticatedIdentity;

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
    pool: SqlitePool,
    crypto: crypto::KeyCipher,
}

#[derive(Debug)]
pub enum StorageError {
    IdentityAlreadyRegistered,
    IdentityNotFound,
    InvalidCredential,
    ProviderNotFound,
    EndpointNotFound,
    ValidationFailed(String),
    InvalidMasterKey(String),
    Crypto(String),
    Database(sqlx::Error),
}

impl StorageError {
    pub const fn code(&self) -> ProtocolErrorCode {
        match self {
            Self::IdentityAlreadyRegistered => ProtocolErrorCode::IdentityAlreadyRegistered,
            Self::InvalidCredential => ProtocolErrorCode::InvalidCredential,
            Self::IdentityNotFound | Self::ProviderNotFound | Self::EndpointNotFound => {
                ProtocolErrorCode::NotFound
            }
            Self::InvalidMasterKey(_) | Self::ValidationFailed(_) => {
                ProtocolErrorCode::ValidationFailed
            }
            Self::Crypto(_) | Self::Database(_) => ProtocolErrorCode::Internal,
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
            Self::ValidationFailed(message) => formatter.write_str(message),
            Self::InvalidMasterKey(message) => write!(formatter, "invalid master key: {message}"),
            Self::Crypto(message) => write!(formatter, "key encryption failed: {message}"),
            Self::Database(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for StorageError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl Storage {
    pub fn from_pool(pool: SqlitePool, master_key: MasterKey) -> Self {
        Self {
            pool,
            crypto: crypto::KeyCipher::new(master_key),
        }
    }

    #[cfg(test)]
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn initialize(pool: SqlitePool, master_key: MasterKey) -> Result<Self, StorageError> {
        schema::initialize(&pool).await?;
        Ok(Self::from_pool(pool, master_key))
    }

    pub async fn in_memory_from_base64(master_key: &str) -> Result<Self, StorageError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::from_str("sqlite::memory:")
                    .expect("valid in-memory SQLite URL")
                    .foreign_keys(true),
            )
            .await?;
        Self::initialize(pool, MasterKey::from_base64(master_key)?).await
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
        identities::register(
            &self.pool,
            machine_id,
            account_sid,
            credential,
            display_name,
        )
        .await
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
        identities::authenticate(&self.pool, credential, display_name).await
    }

    pub async fn rotate_identity_credential(
        &self,
        identity_id: &str,
        authenticated_credential_hash: &str,
        new_credential: &str,
    ) -> Result<RotateCredentialResponse, StorageError> {
        identities::rotate_credential(
            &self.pool,
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
        identities::credential_hash(&self.pool, identity_id).await
    }

    pub async fn delete_inactive_identities(
        &self,
        now: DateTime<Utc>,
        retention: Duration,
    ) -> Result<u64, StorageError> {
        identities::delete_inactive(&self.pool, now, retention).await
    }

    pub async fn create_provider(
        &self,
        identity_id: &str,
        input: CreateProviderRequest,
    ) -> Result<String, StorageError> {
        providers::create(&self.pool, &self.crypto, identity_id, input).await
    }

    pub async fn raw_provider_key_ciphertext(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<String, StorageError> {
        providers::raw_key_ciphertext(&self.pool, identity_id, provider_id).await
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
        providers::list(&self.pool, &self.crypto, identity_id).await
    }

    pub async fn get_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<ProviderResponse, StorageError> {
        providers::get(&self.pool, &self.crypto, identity_id, provider_id).await
    }

    pub async fn update_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
        input: prelay_protocol::providers::UpdateProviderRequest,
    ) -> Result<ProviderResponse, StorageError> {
        providers::update(&self.pool, &self.crypto, identity_id, provider_id, input).await
    }

    pub async fn delete_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<(), StorageError> {
        providers::delete(&self.pool, identity_id, provider_id).await
    }

    pub async fn add_provider_models(
        &self,
        identity_id: &str,
        provider_id: &str,
        model_names: &[String],
    ) -> Result<(), StorageError> {
        providers::add_models(&self.pool, identity_id, provider_id, model_names).await
    }

    pub async fn create_interface(
        &self,
        identity_id: &str,
        input: CreateEndpointRequest,
    ) -> Result<EndpointResponse, StorageError> {
        endpoints::create(&self.pool, identity_id, input).await
    }

    pub async fn list_endpoints(
        &self,
        identity_id: &str,
    ) -> Result<Vec<EndpointResponse>, StorageError> {
        endpoints::list(&self.pool, identity_id).await
    }

    pub async fn get_interface(
        &self,
        identity_id: &str,
        endpoint_id: &str,
    ) -> Result<EndpointResponse, StorageError> {
        endpoints::get(&self.pool, identity_id, endpoint_id).await
    }

    pub async fn update_interface(
        &self,
        identity_id: &str,
        endpoint_id: &str,
        input: prelay_protocol::endpoints::UpdateEndpointRequest,
    ) -> Result<EndpointResponse, StorageError> {
        endpoints::update(&self.pool, identity_id, endpoint_id, input).await
    }

    pub async fn delete_interface(
        &self,
        identity_id: &str,
        endpoint_id: &str,
    ) -> Result<(), StorageError> {
        endpoints::delete(&self.pool, identity_id, endpoint_id).await
    }

    pub async fn regenerate_endpoint_token(
        &self,
        identity_id: &str,
        endpoint_id: &str,
    ) -> Result<EndpointResponse, StorageError> {
        endpoints::regenerate_token(&self.pool, identity_id, endpoint_id).await
    }

    pub async fn authenticate_protocol_access(
        &self,
        token: &str,
    ) -> Result<Option<ProtocolAccess>, StorageError> {
        let access = sqlx::query_as::<_, (String, String, String)>(
            "SELECT identity_id, id, name FROM identity_endpoint_configs WHERE token = ?",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await?
        .map(|(identity_id, endpoint_id, endpoint_name)| ProtocolAccess {
            identity_id,
            endpoint_id,
            endpoint_name,
        });
        if let Some(access) = &access {
            identities::touch(&self.pool, &access.identity_id).await?;
        }
        Ok(access)
    }

    pub async fn resolve_protocol_models(
        &self,
        access: &ProtocolAccess,
        model_name: &str,
    ) -> Result<Vec<ProtocolModel>, StorageError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            model_id: String,
            endpoint_id: String,
            model_name: String,
            provider_id: String,
            upstream_model: String,
            model_created_at: String,
            provider_name: String,
            provider_type: String,
            base_url: String,
            api_key_ciphertext: String,
            capabilities_json: Option<String>,
            provider_created_at: String,
        }
        let rows = sqlx::query_as::<_, Row>(
            "SELECT im.id AS model_id, im.endpoint_id, im.model_name, im.provider_id, \
             im.upstream_model, im.created_at AS model_created_at, p.name AS provider_name, \
             p.provider_type, p.base_url, p.api_key_ciphertext, p.capabilities_json, \
             p.created_at AS provider_created_at \
             FROM identity_endpoint_models im \
             JOIN identity_endpoint_configs i ON i.id = im.endpoint_id \
                 AND i.identity_id = ? \
             JOIN identity_provider_configs p ON p.id = im.provider_id \
                 AND p.identity_id = i.identity_id \
             JOIN identity_provider_models pm ON pm.provider_id = p.id \
                 AND pm.model_name = im.upstream_model \
             WHERE im.endpoint_id = ? AND im.model_name = ? \
             ORDER BY im.candidate_order, im.id",
        )
        .bind(&access.identity_id)
        .bind(&access.endpoint_id)
        .bind(model_name)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ProtocolModel {
                    model: crate::models::EndpointModel {
                        id: row.model_id,
                        endpoint_id: row.endpoint_id,
                        model_name: row.model_name,
                        provider_id: row.provider_id.clone(),
                        upstream_model: row.upstream_model,
                        created_at: row.model_created_at,
                    },
                    provider: crate::models::ProviderConfig {
                        id: row.provider_id,
                        name: row.provider_name,
                        provider_type: row.provider_type,
                        base_url: row.base_url,
                        api_key: self.crypto.decrypt(&row.api_key_ciphertext)?,
                        token: String::new(),
                        capabilities_json: row.capabilities_json,
                        created_at: row.provider_created_at,
                    },
                })
            })
            .collect()
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

        let active_provider_id = sqlx::query_scalar::<_, String>(
            "SELECT provider_id FROM identity_endpoint_model_routes \
             WHERE endpoint_id = ? AND model_name = ?",
        )
        .bind(&access.endpoint_id)
        .bind(model_name)
        .fetch_optional(&self.pool)
        .await?;
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
        let provider_ids = provider_ids.collect::<Vec<_>>();
        if provider_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT provider_id, AVG(upstream_latency_ms) \
             FROM identity_request_logs WHERE identity_id = ",
        );
        query.push_bind(identity_id);
        query.push(
            " AND status = 'success' AND upstream_latency_ms IS NOT NULL AND provider_id IN (",
        );
        {
            let mut separated = query.separated(", ");
            for provider_id in provider_ids {
                separated.push_bind(provider_id);
            }
        }
        query.push(") GROUP BY provider_id");
        let rows = query
            .build_query_as::<(String, f64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    pub async fn remember_protocol_model_provider(
        &self,
        access: &ProtocolAccess,
        model_name: &str,
        provider_id: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO identity_endpoint_model_routes (endpoint_id, model_name, provider_id, updated_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(endpoint_id, model_name) DO UPDATE SET \
             provider_id = excluded.provider_id, updated_at = excluded.updated_at",
        )
        .bind(&access.endpoint_id)
        .bind(model_name)
        .bind(provider_id)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_protocol_models(
        &self,
        access: &ProtocolAccess,
    ) -> Result<Vec<ProtocolModel>, StorageError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            model_id: String,
            endpoint_id: String,
            model_name: String,
            provider_id: String,
            upstream_model: String,
            model_created_at: String,
            provider_name: String,
            provider_type: String,
            base_url: String,
            api_key_ciphertext: String,
            capabilities_json: Option<String>,
            provider_created_at: String,
        }
        let rows = sqlx::query_as::<_, Row>(
            "SELECT im.id AS model_id, im.endpoint_id, im.model_name, im.provider_id, \
             im.upstream_model, im.created_at AS model_created_at, p.name AS provider_name, \
             p.provider_type, p.base_url, p.api_key_ciphertext, p.capabilities_json, \
             p.created_at AS provider_created_at \
             FROM identity_endpoint_models im \
             JOIN identity_endpoint_configs i ON i.id = im.endpoint_id \
                 AND i.identity_id = ? \
             JOIN identity_provider_configs p ON p.id = im.provider_id \
                 AND p.identity_id = i.identity_id \
             JOIN identity_provider_models pm ON pm.provider_id = p.id \
                 AND pm.model_name = im.upstream_model \
             WHERE im.endpoint_id = ? ORDER BY im.candidate_order, im.id",
        )
        .bind(&access.identity_id)
        .bind(&access.endpoint_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ProtocolModel {
                    model: crate::models::EndpointModel {
                        id: row.model_id,
                        endpoint_id: row.endpoint_id,
                        model_name: row.model_name,
                        provider_id: row.provider_id.clone(),
                        upstream_model: row.upstream_model,
                        created_at: row.model_created_at,
                    },
                    provider: crate::models::ProviderConfig {
                        id: row.provider_id,
                        name: row.provider_name,
                        provider_type: row.provider_type,
                        base_url: row.base_url,
                        api_key: self.crypto.decrypt(&row.api_key_ciphertext)?,
                        token: String::new(),
                        capabilities_json: row.capabilities_json,
                        created_at: row.provider_created_at,
                    },
                })
            })
            .collect()
    }
}

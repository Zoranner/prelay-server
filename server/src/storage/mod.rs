mod crypto;
mod identities;
mod schema;

pub mod interfaces;
pub mod providers;
pub mod sessions;
pub mod stats;

use std::fmt;

use chrono::{DateTime, Duration, Utc};

use provider_relay_protocol::{
    CreateIdentityResponse, CreateInterfaceRequest, CreateProviderRequest, InterfaceResponse,
    ProtocolErrorCode, ProviderResponse, RotateCredentialResponse,
};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::str::FromStr;

pub use crypto::MasterKey;
pub use identities::AuthenticatedIdentity;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolAccess {
    pub identity_id: String,
    pub interface_id: String,
}

#[derive(Clone, Debug)]
pub struct ProtocolModel {
    pub model: crate::models::InterfaceModel,
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
    InterfaceNotFound,
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
            Self::IdentityNotFound | Self::ProviderNotFound | Self::InterfaceNotFound => {
                ProtocolErrorCode::NotFound
            }
            Self::InvalidMasterKey(_) | Self::Crypto(_) | Self::ValidationFailed(_) => {
                ProtocolErrorCode::ValidationFailed
            }
            Self::Database(_) => ProtocolErrorCode::Internal,
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
            Self::InterfaceNotFound => formatter.write_str("interface does not exist for identity"),
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
    ) -> Result<CreateIdentityResponse, StorageError> {
        identities::register(&self.pool, machine_id, account_sid).await
    }

    pub async fn authenticate_identity(
        &self,
        credential: &str,
    ) -> Result<Option<AuthenticatedIdentity>, StorageError> {
        identities::authenticate(&self.pool, credential).await
    }

    pub async fn rotate_identity_credential(
        &self,
        identity_id: &str,
        authenticated_credential_hash: &str,
    ) -> Result<RotateCredentialResponse, StorageError> {
        identities::rotate_credential(&self.pool, identity_id, authenticated_credential_hash).await
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
        providers::list(&self.pool, identity_id).await
    }

    pub async fn get_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<ProviderResponse, StorageError> {
        providers::get(&self.pool, identity_id, provider_id).await
    }

    pub async fn update_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
        input: provider_relay_protocol::providers::UpdateProviderRequest,
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

    pub async fn create_interface(
        &self,
        identity_id: &str,
        input: CreateInterfaceRequest,
    ) -> Result<InterfaceResponse, StorageError> {
        interfaces::create(&self.pool, identity_id, input).await
    }

    pub async fn list_interfaces(
        &self,
        identity_id: &str,
    ) -> Result<Vec<InterfaceResponse>, StorageError> {
        interfaces::list(&self.pool, identity_id).await
    }

    pub async fn get_interface(
        &self,
        identity_id: &str,
        interface_id: &str,
    ) -> Result<InterfaceResponse, StorageError> {
        interfaces::get(&self.pool, identity_id, interface_id).await
    }

    pub async fn update_interface(
        &self,
        identity_id: &str,
        interface_id: &str,
        input: provider_relay_protocol::interfaces::UpdateInterfaceRequest,
    ) -> Result<InterfaceResponse, StorageError> {
        interfaces::update(&self.pool, identity_id, interface_id, input).await
    }

    pub async fn delete_interface(
        &self,
        identity_id: &str,
        interface_id: &str,
    ) -> Result<(), StorageError> {
        interfaces::delete(&self.pool, identity_id, interface_id).await
    }

    pub async fn regenerate_interface_token(
        &self,
        identity_id: &str,
        interface_id: &str,
    ) -> Result<InterfaceResponse, StorageError> {
        interfaces::regenerate_token(&self.pool, identity_id, interface_id).await
    }

    pub async fn authenticate_protocol_access(
        &self,
        token: &str,
    ) -> Result<Option<ProtocolAccess>, StorageError> {
        let access = sqlx::query_as::<_, (String, String)>(
            "SELECT identity_id, id FROM identity_interface_configs WHERE token = ?",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await?
        .map(|(identity_id, interface_id)| ProtocolAccess {
            identity_id,
            interface_id,
        });
        if let Some(access) = &access {
            identities::touch(&self.pool, &access.identity_id).await?;
        }
        Ok(access)
    }

    pub async fn resolve_protocol_model(
        &self,
        access: &ProtocolAccess,
        model_name: &str,
    ) -> Result<Option<ProtocolModel>, StorageError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            model_id: String,
            interface_id: String,
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
        let row = sqlx::query_as::<_, Row>(
            "SELECT im.id AS model_id, im.interface_id, im.model_name, im.provider_id, \
             im.upstream_model, im.created_at AS model_created_at, p.name AS provider_name, \
             p.provider_type, p.base_url, p.api_key_ciphertext, p.capabilities_json, \
             p.created_at AS provider_created_at \
             FROM identity_interface_models im \
             JOIN identity_interface_configs i ON i.id = im.interface_id \
                 AND i.identity_id = ? \
             JOIN identity_provider_configs p ON p.id = im.provider_id \
                 AND p.identity_id = i.identity_id \
             JOIN identity_provider_models pm ON pm.provider_id = p.id \
                 AND pm.model_name = im.upstream_model \
             WHERE im.interface_id = ? AND im.model_name = ?",
        )
        .bind(&access.identity_id)
        .bind(&access.interface_id)
        .bind(model_name)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ProtocolModel {
                model: crate::models::InterfaceModel {
                    id: row.model_id,
                    interface_id: row.interface_id,
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
        .transpose()
    }

    pub async fn list_protocol_models(
        &self,
        access: &ProtocolAccess,
    ) -> Result<Vec<ProtocolModel>, StorageError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            model_id: String,
            interface_id: String,
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
            "SELECT im.id AS model_id, im.interface_id, im.model_name, im.provider_id, \
             im.upstream_model, im.created_at AS model_created_at, p.name AS provider_name, \
             p.provider_type, p.base_url, p.api_key_ciphertext, p.capabilities_json, \
             p.created_at AS provider_created_at \
             FROM identity_interface_models im \
             JOIN identity_interface_configs i ON i.id = im.interface_id \
                 AND i.identity_id = ? \
             JOIN identity_provider_configs p ON p.id = im.provider_id \
                 AND p.identity_id = i.identity_id \
             JOIN identity_provider_models pm ON pm.provider_id = p.id \
                 AND pm.model_name = im.upstream_model \
             WHERE im.interface_id = ? ORDER BY im.created_at",
        )
        .bind(&access.identity_id)
        .bind(&access.interface_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ProtocolModel {
                    model: crate::models::InterfaceModel {
                        id: row.model_id,
                        interface_id: row.interface_id,
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

mod crypto;
mod identities;
mod schema;

pub mod interfaces;
pub mod providers;
pub mod sessions;
pub mod stats;

use std::fmt;

use provider_relay_protocol::{
    CreateIdentityResponse, CreateInterfaceRequest, CreateProviderRequest, InterfaceResponse,
    ProtocolErrorCode, ProviderResponse, RotateCredentialResponse,
};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

pub use crypto::MasterKey;

#[derive(Clone)]
pub struct Storage {
    pool: SqlitePool,
    crypto: crypto::KeyCipher,
}

#[derive(Debug)]
pub enum StorageError {
    IdentityAlreadyRegistered,
    IdentityNotFound,
    ProviderNotFound,
    InterfaceNotFound,
    InvalidMasterKey(String),
    Crypto(String),
    Database(sqlx::Error),
}

impl StorageError {
    pub const fn code(&self) -> ProtocolErrorCode {
        match self {
            Self::IdentityAlreadyRegistered => ProtocolErrorCode::IdentityAlreadyRegistered,
            Self::IdentityNotFound | Self::ProviderNotFound | Self::InterfaceNotFound => {
                ProtocolErrorCode::NotFound
            }
            Self::InvalidMasterKey(_) | Self::Crypto(_) => ProtocolErrorCode::ValidationFailed,
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
            Self::ProviderNotFound => formatter.write_str("provider does not exist for identity"),
            Self::InterfaceNotFound => formatter.write_str("interface does not exist for identity"),
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

    pub async fn initialize(pool: SqlitePool, master_key: MasterKey) -> Result<Self, StorageError> {
        schema::initialize(&pool).await?;
        Ok(Self::from_pool(pool, master_key))
    }

    pub async fn in_memory_from_base64(master_key: &str) -> Result<Self, StorageError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
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
    ) -> Result<Option<String>, StorageError> {
        identities::authenticate(&self.pool, credential).await
    }

    pub async fn rotate_identity_credential(
        &self,
        identity_id: &str,
    ) -> Result<RotateCredentialResponse, StorageError> {
        identities::rotate_credential(&self.pool, identity_id).await
    }

    pub async fn identity_credential_hash(
        &self,
        identity_id: &str,
    ) -> Result<String, StorageError> {
        identities::credential_hash(&self.pool, identity_id).await
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
}

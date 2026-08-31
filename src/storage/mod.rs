mod access;
mod activities;
mod crypto;
mod identities;
mod memories;
mod sessions;
mod stats;

pub mod endpoints;
pub mod providers;

use std::fmt;

use prelay_protocol::ProtocolErrorCode;
use sea_orm::{DatabaseConnection, DbErr};

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
    ActivityNotFound,
    MemoryNotFound,
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
            | Self::ActivityNotFound
            | Self::MemoryNotFound => ProtocolErrorCode::NotFound,
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
            Self::ActivityNotFound => formatter.write_str("activity does not exist for identity"),
            Self::MemoryNotFound => formatter.write_str("memory does not exist"),
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
}

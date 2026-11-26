mod access;
mod activities;
mod activity_materialization;
mod crypto;
mod endpoint_validation;
mod error;
mod identities;
mod leaderboard;
mod memories;
mod provider_quota;
mod provider_usage;
mod provider_validation;
mod provider_views;
mod provider_visibility;
mod response_states;
mod rollout_recovery;
mod rollouts;
mod sessions;
mod stats;

pub mod endpoints;
pub mod providers;

use std::fmt;

use prelay_protocol::ProtocolErrorCode;
use sea_orm::{DatabaseConnection, DbErr};

use crate::domain::{
    activity::rollout::RolloutSummary,
    stats::{ActivityInsert, StreamActivityUpdate},
};

pub use crypto::MasterKey;
pub use identities::{AuthenticatedIdentity, DeleteInactiveResult};
pub(crate) use provider_validation::parse_models;
pub(crate) use provider_visibility::parse_share_quota;
pub use response_states::{ResponseState, ResponseStateInsert};
pub use rollout_recovery::{InterruptedRollout, RecoveredRollout};

pub(crate) struct ActivityRolloutMaterialization {
    pub(crate) identity_id: String,
    pub(crate) activity_id: String,
    pub(crate) activity: ActivityInsert,
    pub(crate) activity_created_at: String,
    pub(crate) rollout: RolloutSummary,
    pub(crate) rollout_created_at: String,
    pub(crate) completed_at: Option<String>,
}

pub(crate) struct CompletedStreamMaterialization {
    pub(crate) identity_id: String,
    pub(crate) activity_id: String,
    pub(crate) activity: StreamActivityUpdate,
    /// 活动行缺失时用于补建该行的活动快照。
    pub(crate) activity_snapshot: Option<ActivityInsert>,
    pub(crate) rollout: RolloutSummary,
    pub(crate) rollout_created_at: String,
    pub(crate) completed_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolAccess {
    pub identity_id: String,
    pub endpoint_id: String,
    pub endpoint_name: String,
}

#[derive(Clone, Debug)]
pub struct ProtocolModel {
    pub model: crate::domain::registry::EndpointModel,
    pub provider: crate::domain::registry::ProviderConfig,
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
    InvalidProviderSharing(String),
    ProviderSharingNotAllowed,
    ProviderNotVisible,
    ProviderNotUsable,
    ProviderRouteUnavailable,
    ProviderModelRetired(String),
    ProviderTypeUnsupported(String),
    EndpointModelDuplicate,
    EndpointModelNotInProviderModels(String),
    EndpointNotFound,
    ActivityNotFound,
    MemoryNotFound,
    InvalidStoredState(String),
    ValidationFailed(String),
    InvalidTimestamp(String),
    InvalidMasterKey(String),
    Crypto(String),
    Recording(String),
    Rollout(String),
    Serialization(serde_json::Error),
    Database(DbErr),
}

impl StorageError {
    pub const fn code(&self) -> ProtocolErrorCode {
        match self {
            Self::IdentityAlreadyRegistered => ProtocolErrorCode::IdentityAlreadyRegistered,
            Self::InvalidCredential => ProtocolErrorCode::InvalidCredential,
            Self::InvalidProviderSharing(_) => ProtocolErrorCode::InvalidProviderSharing,
            Self::ProviderSharingNotAllowed => ProtocolErrorCode::ProviderSharingNotAllowed,
            Self::ProviderNotVisible => ProtocolErrorCode::ProviderNotVisible,
            Self::ProviderNotUsable => ProtocolErrorCode::ProviderNotUsable,
            Self::ProviderRouteUnavailable => ProtocolErrorCode::ProviderRouteUnavailable,
            Self::ProviderModelRetired(_) => ProtocolErrorCode::ProviderModelRetired,
            Self::ProviderTypeUnsupported(_) => ProtocolErrorCode::ProviderTypeUnsupported,
            Self::EndpointModelDuplicate => ProtocolErrorCode::EndpointModelDuplicate,
            Self::EndpointModelNotInProviderModels(_) => {
                ProtocolErrorCode::EndpointModelNotInProviderModels
            }
            Self::IdentityNotFound
            | Self::ProviderNotFound
            | Self::EndpointNotFound
            | Self::ActivityNotFound
            | Self::MemoryNotFound => ProtocolErrorCode::NotFound,
            Self::InvalidMasterKey(_) | Self::ValidationFailed(_) => {
                ProtocolErrorCode::ValidationFailed
            }
            Self::InvalidTimestamp(_)
            | Self::InvalidStoredState(_)
            | Self::Crypto(_)
            | Self::Recording(_)
            | Self::Rollout(_)
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
            Self::InvalidStoredState(message) => {
                write!(formatter, "invalid stored response state: {message}")
            }
            Self::ValidationFailed(message) => formatter.write_str(message),
            Self::InvalidTimestamp(message) => {
                write!(formatter, "invalid stored timestamp: {message}")
            }
            Self::InvalidMasterKey(message) => write!(formatter, "invalid master key: {message}"),
            Self::InvalidProviderSharing(message) => {
                write!(formatter, "invalid provider sharing: {message}")
            }
            Self::ProviderSharingNotAllowed => {
                formatter.write_str("provider sharing can only be managed by its owner")
            }
            Self::ProviderNotVisible => formatter.write_str("provider is not visible to identity"),
            Self::ProviderNotUsable => formatter.write_str("provider is not usable by identity"),
            Self::ProviderRouteUnavailable => {
                formatter.write_str("provider route is no longer available")
            }
            Self::ProviderModelRetired(detail) => {
                write!(
                    formatter,
                    "provider models are no longer in the catalog: {detail}"
                )
            }
            Self::ProviderTypeUnsupported(detail) => {
                write!(formatter, "provider type is not usable: {detail}")
            }
            Self::EndpointModelDuplicate => {
                formatter.write_str("endpoint model is already bound to the same provider")
            }
            Self::EndpointModelNotInProviderModels(detail) => {
                write!(
                    formatter,
                    "endpoint model is not enabled by the provider: {detail}"
                )
            }
            Self::Crypto(message) => write!(formatter, "key encryption failed: {message}"),
            Self::Recording(message) => write!(formatter, "recording is unavailable: {message}"),
            Self::Rollout(message) => write!(formatter, "rollout storage failed: {message}"),
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

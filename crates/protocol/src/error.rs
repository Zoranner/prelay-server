use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolErrorCode {
    IdentityAlreadyRegistered,
    InvalidCredential,
    NotFound,
    ValidationFailed,
    Internal,
}

impl ProtocolErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdentityAlreadyRegistered => "identity_already_registered",
            Self::InvalidCredential => "invalid_credential",
            Self::NotFound => "not_found",
            Self::ValidationFailed => "validation_failed",
            Self::Internal => "internal",
        }
    }
}

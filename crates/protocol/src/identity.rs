use serde::{Deserialize, Serialize};

pub const DEVICE_CREDENTIAL_BYTES: usize = 32;
pub const DEVICE_CREDENTIAL_URL_SAFE_BASE64_LENGTH: usize = 43;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateIdentityRequest {
    pub machine_id: String,
    pub account_sid: String,
    pub credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateIdentityResponse {
    pub identity_id: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RotateCredentialRequest {
    pub new_credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RotateCredentialResponse {
    pub rotated: bool,
}

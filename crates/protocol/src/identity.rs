use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateIdentityRequest {
    pub machine_id: String,
    pub account_sid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateIdentityResponse {
    pub identity_id: String,
    pub credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RotateCredentialResponse {
    pub credential: String,
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterfaceModelInput {
    pub provider_id: String,
    pub upstream_model: String,
    pub model_name: Option<String>,
}

impl InterfaceModelInput {
    pub fn default_model_name(upstream_model: &str) -> String {
        upstream_model.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateInterfaceRequest {
    pub name: String,
    pub protocol: Option<String>,
    pub models: Vec<InterfaceModelInput>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateInterfaceRequest {
    pub name: Option<String>,
    pub protocol: Option<String>,
    pub models: Option<Vec<InterfaceModelInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterfaceModelResponse {
    pub id: String,
    pub interface_id: String,
    pub model_name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterfaceResponse {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub token: String,
    pub models: Vec<InterfaceModelResponse>,
    pub created_at: String,
}

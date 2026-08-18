use std::fmt;

use provider_relay_protocol::{CreateIdentityRequest, CreateIdentityResponse};
use reqwest::{header::AUTHORIZATION, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{credential_store::CredentialStore, identity::WindowsIdentity};

pub const DEFAULT_RELAY_URL: &str = "https://relay.rd.kim";

#[derive(Clone, Debug, Serialize)]
pub struct ClientError {
    pub code: String,
    pub message: String,
}

impl ClientError {
    pub const MISSING_DEVICE_CREDENTIAL: &'static str = "missing_device_credential";

    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ClientError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRequest {
    url: String,
    authorization: String,
}

#[derive(Default)]
pub struct RegistrationGate(tokio::sync::Mutex<()>);

impl PreparedRequest {
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn authorization(&self) -> &str {
        &self.authorization
    }
}

pub struct ApiClient<'a> {
    base_url: String,
    credential_store: &'a dyn CredentialStore,
    http: reqwest::Client,
}

impl<'a> ApiClient<'a> {
    pub fn new(
        base_url: impl AsRef<str>,
        credential_store: &'a dyn CredentialStore,
    ) -> Result<Self, ClientError> {
        let base_url = normalize_base_url(base_url.as_ref())?;
        Ok(Self {
            base_url,
            credential_store,
            http: reqwest::Client::new(),
        })
    }

    pub fn from_environment(
        credential_store: &'a dyn CredentialStore,
    ) -> Result<Self, ClientError> {
        Self::new(configured_relay_url()?, credential_store)
    }

    pub fn has_stored_credential(&self) -> Result<bool, ClientError> {
        Ok(self
            .credential_store
            .load()
            .map_err(credential_store_error)?
            .is_some_and(|credential| !credential.trim().is_empty()))
    }

    pub fn authenticated_request(
        &self,
        method: &str,
        path: &str,
    ) -> Result<PreparedRequest, ClientError> {
        Method::from_bytes(method.as_bytes()).map_err(|_| {
            ClientError::new("invalid_request", "management request method is invalid")
        })?;
        let credential = self.load_credential()?;
        Ok(PreparedRequest {
            url: self.url(path)?,
            authorization: format!("Bearer {credential}"),
        })
    }

    pub async fn ensure_registered(&self, identity: &WindowsIdentity) -> Result<(), ClientError> {
        if self.has_stored_credential()? {
            return Ok(());
        }

        let response: CreateIdentityResponse = self
            .send_json(
                Method::POST,
                "/api/identities",
                Some(&CreateIdentityRequest {
                    machine_id: identity.machine_id.clone(),
                    account_sid: identity.account_sid.clone(),
                }),
                None,
            )
            .await?;
        self.credential_store
            .save(&response.credential)
            .map_err(credential_store_error)
    }

    pub async fn ensure_registered_once(
        &self,
        identity: &WindowsIdentity,
        gate: &RegistrationGate,
    ) -> Result<(), ClientError> {
        let _guard = gate.0.lock().await;
        self.ensure_registered(identity).await
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        self.send_json::<T, ()>(Method::GET, path, None, Some(self.load_credential()?))
            .await
    }

    pub async fn post<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        self.send_json(
            Method::POST,
            path,
            Some(body),
            Some(self.load_credential()?),
        )
        .await
    }

    pub async fn patch<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        self.send_json(
            Method::PATCH,
            path,
            Some(body),
            Some(self.load_credential()?),
        )
        .await
    }

    pub async fn delete(&self, path: &str) -> Result<(), ClientError> {
        let credential = self.load_credential()?;
        let response = self
            .http
            .delete(self.url(path)?)
            .header(AUTHORIZATION, format!("Bearer {credential}"))
            .send()
            .await
            .map_err(network_error)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(response_error(response).await)
        }
    }

    fn load_credential(&self) -> Result<String, ClientError> {
        self.credential_store
            .load()
            .map_err(credential_store_error)?
            .filter(|credential| !credential.trim().is_empty())
            .ok_or_else(|| {
                ClientError::new(
                    ClientError::MISSING_DEVICE_CREDENTIAL,
                    "device credential is unavailable; identity cannot be restored automatically",
                )
            })
    }

    async fn send_json<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        credential: Option<String>,
    ) -> Result<T, ClientError> {
        let mut request = self.http.request(method, self.url(path)?);
        if let Some(credential) = credential {
            request = request.header(AUTHORIZATION, format!("Bearer {credential}"));
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(network_error)?;
        if !response.status().is_success() {
            return Err(response_error(response).await);
        }
        response.json().await.map_err(|_| {
            ClientError::new(
                "invalid_response",
                "relay returned an invalid management response",
            )
        })
    }

    fn url(&self, path: &str) -> Result<String, ClientError> {
        if !path.starts_with('/') {
            return Err(ClientError::new(
                "invalid_request",
                "management request path must be absolute",
            ));
        }
        Ok(format!("{}{}", self.base_url, path))
    }
}

pub fn configured_relay_url() -> Result<String, ClientError> {
    let base_url =
        std::env::var("PROVIDER_RELAY_URL").unwrap_or_else(|_| DEFAULT_RELAY_URL.to_string());
    normalize_base_url(&base_url)
}

fn normalize_base_url(value: &str) -> Result<String, ClientError> {
    let value = value.trim().trim_end_matches('/');
    if !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err(ClientError::new(
            "invalid_relay_url",
            "PROVIDER_RELAY_URL must be an HTTP or HTTPS URL",
        ));
    }
    Ok(value.to_owned())
}

fn credential_store_error(error: String) -> ClientError {
    ClientError::new("credential_store_error", error)
}

fn network_error(_: reqwest::Error) -> ClientError {
    ClientError::new("network_error", "unable to reach the relay management API")
}

async fn response_error(response: reqwest::Response) -> ClientError {
    let status = response.status();
    let (server_code, server_message) = if status.is_client_error() {
        response
            .json::<ServerErrorEnvelope>()
            .await
            .ok()
            .map(|body| body.error.into_parts())
            .unwrap_or_default()
    } else {
        (None, None)
    };
    let code = server_code.unwrap_or_else(|| status_code(status).to_owned());
    let message = server_message.unwrap_or_else(|| {
        if code == "identity_already_registered" {
            "this Windows identity is already registered and cannot be restored automatically"
                .into()
        } else {
            "relay rejected the management request".into()
        }
    });
    ClientError::new(code, message)
}

fn status_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "invalid_credential",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => "validation_failed",
        _ => "internal",
    }
}

#[derive(Deserialize)]
struct ServerErrorEnvelope {
    error: ServerErrorBody,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ServerErrorBody {
    Structured {
        code: Option<String>,
        message: Option<String>,
    },
    Message(String),
}

impl ServerErrorBody {
    fn into_parts(self) -> (Option<String>, Option<String>) {
        match self {
            Self::Structured { code, message } => {
                (code, message.filter(|message| !message.trim().is_empty()))
            }
            Self::Message(message) => (None, (!message.trim().is_empty()).then_some(message)),
        }
    }
}

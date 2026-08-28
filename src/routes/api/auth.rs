use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

use crate::{error::AppError, AppState};

#[derive(Clone, Debug)]
pub struct CurrentIdentity {
    pub id: String,
    pub credential_hash: String,
}

pub async fn require_device_credential(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let credential = extract_bearer_credential(request.headers()).ok_or(AppError::Unauthorized)?;
    let display_name = extract_display_name(request.headers());
    let identity = state
        .storage
        .authenticate_identity_with_display_name(&credential, display_name.as_deref())
        .await?
        .ok_or(AppError::Unauthorized)?;
    request.extensions_mut().insert(CurrentIdentity {
        id: identity.id,
        credential_hash: identity.credential_hash,
    });
    Ok(next.run(request).await)
}

pub(crate) fn extract_display_name(headers: &HeaderMap) -> Option<String> {
    let encoded = headers
        .get("x-prelay-display-name")
        .and_then(|value| value.to_str().ok())?;
    let display_name = String::from_utf8(URL_SAFE_NO_PAD.decode(encoded).ok()?).ok()?;
    (!display_name.trim().is_empty()).then(|| display_name.trim().to_string())
}

fn extract_bearer_credential(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|credential| !credential.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::extract_display_name;

    #[test]
    fn extracts_base64_encoded_display_name() {
        let mut headers = HeaderMap::new();
        headers.insert("x-prelay-display-name", "5L2g5aW9".parse().unwrap());

        assert_eq!(extract_display_name(&headers).as_deref(), Some("你好"));
    }
}

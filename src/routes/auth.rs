use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use sqlx::SqlitePool;

use crate::{error::AppError, AppState};

pub async fn require_protocol_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    authenticate_protocol_request(&state.db, request.headers()).await?;
    Ok(next.run(request).await)
}

pub async fn authenticate_protocol_request(
    db: &SqlitePool,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    let token = extract_token(headers).ok_or(AppError::Unauthorized)?;
    db::get_config_by_token(db, &token)
        .await?
        .ok_or(AppError::Unauthorized)?;
    Ok(())
}

pub fn extract_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string)
        })
}

use crate::db;

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};
    use sqlx::sqlite::SqlitePoolOptions;

    use super::{authenticate_protocol_request, extract_token};
    use crate::db;

    #[test]
    fn extracts_bearer_token_from_authorization_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer proxy-token"),
        );

        assert_eq!(extract_token(&headers).as_deref(), Some("proxy-token"));
    }

    #[test]
    fn extracts_token_from_x_api_key_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("proxy-token"));

        assert_eq!(extract_token(&headers).as_deref(), Some("proxy-token"));
    }

    #[tokio::test]
    async fn authenticates_existing_proxy_token() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {}", provider.token)).expect("header value"),
        );

        authenticate_protocol_request(&db, &headers)
            .await
            .expect("authenticate request");
    }
}

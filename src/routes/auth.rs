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

pub async fn require_admin_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let Some(expected_token) = state.admin_token.as_deref() else {
        return Ok(next.run(request).await);
    };
    authenticate_admin_request(request.headers(), expected_token)?;
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

pub fn authenticate_admin_request(
    headers: &HeaderMap,
    expected_token: &str,
) -> Result<(), AppError> {
    let token = extract_token(headers).ok_or(AppError::Unauthorized)?;
    if token == expected_token {
        Ok(())
    } else {
        Err(AppError::Unauthorized)
    }
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
    use axum::{
        http::{HeaderMap, HeaderValue},
        middleware, Router,
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::net::TcpListener;

    use super::{
        authenticate_admin_request, authenticate_protocol_request, extract_token,
        require_admin_auth,
    };
    use crate::{db, AppState};

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

    #[test]
    fn authenticates_matching_admin_token() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("admin-secret"));

        authenticate_admin_request(&headers, "admin-secret").expect("authenticate admin request");
    }

    #[test]
    fn rejects_missing_or_wrong_admin_token() {
        let headers = HeaderMap::new();
        assert!(authenticate_admin_request(&headers, "admin-secret").is_err());

        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer wrong"));
        assert!(authenticate_admin_request(&headers, "admin-secret").is_err());
    }

    #[tokio::test]
    async fn protects_admin_router_when_admin_token_is_configured() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: Some("admin-secret".to_string()),
        };
        let admin_router = crate::routes::admin::router()
            .merge(crate::routes::stats::router())
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(state, require_admin_auth));
        let app = Router::new().nest("/api", admin_router);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let client = reqwest::Client::new();
        let unauthorized = client
            .get(format!("http://{addr}/api/configs"))
            .send()
            .await
            .expect("send unauthorized request");
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

        let authorized = client
            .get(format!("http://{addr}/api/configs"))
            .bearer_auth("admin-secret")
            .send()
            .await
            .expect("send authorized request");
        assert_eq!(authorized.status(), reqwest::StatusCode::OK);

        server.abort();
    }
}

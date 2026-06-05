use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::{error::AppError, stats, AppState};

const DEFAULT_REQUESTS_LIMIT: usize = 50;
const MAX_REQUESTS_LIMIT: usize = 200;

#[derive(Debug, Default, Deserialize)]
struct RequestsQuery {
    limit: Option<usize>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stats/overview", get(overview))
        .route("/stats/requests", get(requests))
        .route("/stats/models", get(models))
        .route("/stats/providers", get(providers))
}

async fn overview(State(state): State<AppState>) -> Result<Json<stats::StatsOverview>, AppError> {
    Ok(Json(stats::overview(&state.db).await?))
}

async fn requests(
    State(state): State<AppState>,
    query: Option<Query<RequestsQuery>>,
) -> Result<Json<Vec<stats::RequestLogSummary>>, AppError> {
    let limit = query
        .map(|Query(query)| query.limit.unwrap_or(DEFAULT_REQUESTS_LIMIT))
        .unwrap_or(DEFAULT_REQUESTS_LIMIT)
        .min(MAX_REQUESTS_LIMIT);

    Ok(Json(stats::list_requests(&state.db, limit).await?))
}

async fn models(
    State(state): State<AppState>,
) -> Result<Json<Vec<stats::ModelStatsSummary>>, AppError> {
    Ok(Json(stats::list_model_stats(&state.db).await?))
}

async fn providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<stats::ProviderStatsSummary>>, AppError> {
    Ok(Json(stats::list_provider_stats(&state.db).await?))
}

#[cfg(test)]
mod tests {
    use axum::extract::State;
    use axum::Router;
    use sqlx::sqlite::SqlitePoolOptions;

    use super::overview;
    use crate::{db, AppState};

    #[tokio::test]
    async fn overview_api_returns_request_log_counts_and_token_totals() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                id,
                created_at,
                status,
                input_tokens,
                output_tokens
            )
            VALUES
                ('log-success', '2026-06-05T00:00:00Z', 'success', 7, 11),
                ('log-failure', '2026-06-05T00:01:00Z', 'failed', 13, 0)
            "#,
        )
        .execute(&db)
        .await
        .expect("insert request logs");

        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };

        let response = overview(State(state)).await.expect("load overview");

        assert_eq!(response.0.total_requests, 2);
        assert_eq!(response.0.successful_requests, 1);
        assert_eq!(response.0.failed_requests, 1);
        assert_eq!(response.0.input_tokens, 20);
        assert_eq!(response.0.output_tokens, 11);
    }

    #[tokio::test]
    async fn requests_api_returns_request_logs_ordered_by_created_at_desc() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                id,
                created_at,
                protocol_in,
                protocol_upstream,
                provider_name,
                model_requested,
                status,
                http_status,
                error_code,
                error_message,
                input_tokens,
                output_tokens,
                latency_ms
            )
            VALUES
                ('older-log', '2026-06-05T00:00:00Z', 'openai', 'anthropic',
                 'Provider One', 'gpt-4o-mini', 'success', 200, NULL, NULL, 7, 11, 120),
                ('newer-log', '2026-06-05T00:01:00Z', 'responses', 'openai',
                 'Provider Two', 'gpt-4.1-mini', 'failed', 502, 'upstream_error',
                 'upstream failed', 13, 0, 300)
            "#,
        )
        .execute(&db)
        .await
        .expect("insert request logs");

        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::get(format!("http://{addr}/api/stats/requests"))
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        let rows = body.as_array().expect("response is an array");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "newer-log");
        assert_eq!(rows[0]["created_at"], "2026-06-05T00:01:00Z");
        assert_eq!(rows[0]["protocol_in"], "responses");
        assert_eq!(rows[0]["protocol_upstream"], "openai");
        assert_eq!(rows[0]["provider_name"], "Provider Two");
        assert_eq!(rows[0]["model_requested"], "gpt-4.1-mini");
        assert_eq!(rows[0]["status"], "failed");
        assert_eq!(rows[0]["http_status"], 502);
        assert_eq!(rows[0]["error_code"], "upstream_error");
        assert_eq!(rows[0]["error_message"], "upstream failed");
        assert_eq!(rows[0]["input_tokens"], 13);
        assert_eq!(rows[0]["output_tokens"], 0);
        assert_eq!(rows[0]["latency_ms"], 300);
        assert_eq!(rows[1]["id"], "older-log");

        server.abort();
    }

    #[tokio::test]
    async fn requests_api_caps_limit_query_at_two_hundred() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        for index in 0..201 {
            sqlx::query(
                r#"
                INSERT INTO request_logs (
                    id,
                    created_at,
                    status
                )
                VALUES (?, ?, 'success')
                "#,
            )
            .bind(format!("log-{index:03}"))
            .bind(format!("2026-06-05T00:{index:02}:00Z"))
            .execute(&db)
            .await
            .expect("insert request log");
        }

        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::get(format!("http://{addr}/api/stats/requests?limit=500"))
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        let rows = body.as_array().expect("response is an array");

        assert_eq!(rows.len(), 200);

        server.abort();
    }

    #[tokio::test]
    async fn requests_api_uses_default_limit_of_fifty() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        for index in 0..51 {
            sqlx::query(
                r#"
                INSERT INTO request_logs (
                    id,
                    created_at,
                    status
                )
                VALUES (?, ?, 'success')
                "#,
            )
            .bind(format!("log-{index:03}"))
            .bind(format!("2026-06-05T00:{index:02}:00Z"))
            .execute(&db)
            .await
            .expect("insert request log");
        }

        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::get(format!("http://{addr}/api/stats/requests"))
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        let rows = body.as_array().expect("response is an array");

        assert_eq!(rows.len(), 50);

        server.abort();
    }

    #[tokio::test]
    async fn models_api_returns_model_aggregates() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        insert_aggregate_request_logs(&db).await;

        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::get(format!("http://{addr}/api/stats/models"))
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        let rows = body.as_array().expect("response is an array");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["model_requested"], "deepseek-chat");
        assert_eq!(rows[0]["total_requests"], 2);
        assert_eq!(rows[0]["failed_requests"], 1);
        assert_eq!(rows[0]["average_latency_ms"], 150.0);

        server.abort();
    }

    #[tokio::test]
    async fn providers_api_returns_provider_aggregates() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        insert_aggregate_request_logs(&db).await;

        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::get(format!("http://{addr}/api/stats/providers"))
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        let rows = body.as_array().expect("response is an array");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["provider_id"], "provider-1");
        assert_eq!(rows[0]["provider_name"], "Provider One");
        assert_eq!(rows[0]["total_requests"], 2);
        assert_eq!(rows[0]["average_first_token_ms"], 50.0);

        server.abort();
    }

    async fn insert_aggregate_request_logs(db: &sqlx::SqlitePool) {
        sqlx::query(
            r#"
            INSERT INTO request_logs (
                id,
                created_at,
                provider_id,
                provider_name,
                model_requested,
                status,
                input_tokens,
                output_tokens,
                estimated_cost,
                latency_ms,
                first_token_ms
            )
            VALUES
                ('log-1', '2026-06-05T00:00:00Z', 'provider-1', 'Provider One',
                 'deepseek-chat', 'success', 12, 5, 0.000012, 100, 50),
                ('log-2', '2026-06-05T00:01:00Z', 'provider-1', 'Provider One',
                 'deepseek-chat', 'failed', 5, 0, NULL, 200, NULL),
                ('log-3', '2026-06-05T00:02:00Z', 'provider-2', 'Provider Two',
                 'kimi-k2', 'success', 7, 9, 0.000034, 300, 120)
            "#,
        )
        .execute(db)
        .await
        .expect("insert aggregate request logs");
    }
}

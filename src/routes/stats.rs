use axum::{extract::State, routing::get, Json, Router};

use crate::{error::AppError, stats, AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/stats/overview", get(overview))
}

async fn overview(State(state): State<AppState>) -> Result<Json<stats::StatsOverview>, AppError> {
    Ok(Json(stats::overview(&state.db).await?))
}

#[cfg(test)]
mod tests {
    use axum::extract::State;
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
        };

        let response = overview(State(state)).await.expect("load overview");

        assert_eq!(response.0.total_requests, 2);
        assert_eq!(response.0.successful_requests, 1);
        assert_eq!(response.0.failed_requests, 1);
        assert_eq!(response.0.input_tokens, 20);
        assert_eq!(response.0.output_tokens, 11);
    }
}

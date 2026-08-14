use provider_relay_protocol::{
    ModelStatsSummary, ProviderStatsSummary, RequestLogSummary, StatsOverview,
};
use tauri::State;

use crate::{api_client::ClientError, commands::authenticated_api, NativeState};

#[tauri::command]
pub async fn stats_overview(state: State<'_, NativeState>) -> Result<StatsOverview, ClientError> {
    authenticated_api(&state)
        .await?
        .get("/api/stats/overview")
        .await
}

#[tauri::command]
pub async fn stats_requests(
    state: State<'_, NativeState>,
    limit: Option<usize>,
) -> Result<Vec<RequestLogSummary>, ClientError> {
    let path = match limit {
        Some(limit) => format!("/api/stats/requests?limit={limit}"),
        None => "/api/stats/requests".to_string(),
    };
    authenticated_api(&state).await?.get(&path).await
}

#[tauri::command]
pub async fn stats_models(
    state: State<'_, NativeState>,
) -> Result<Vec<ModelStatsSummary>, ClientError> {
    authenticated_api(&state)
        .await?
        .get("/api/stats/models")
        .await
}

#[tauri::command]
pub async fn stats_providers(
    state: State<'_, NativeState>,
) -> Result<Vec<ProviderStatsSummary>, ClientError> {
    authenticated_api(&state)
        .await?
        .get("/api/stats/providers")
        .await
}

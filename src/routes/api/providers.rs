#[path = "provider_protocol_test.rs"]
mod provider_protocol_test;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use prelay_protocol::{
    CreateProviderRequest, ProviderListItemResponse, ProviderOperationRequest,
    ProviderOperationResponse, ProviderResponse, ProviderUsageResponse,
    TestProviderProtocolRequest, UpdateProviderRequest,
};
use serde::Deserialize;

use crate::{providers::model_discovery, stats::StatsRange, AppState};

use super::auth::CurrentIdentity;
use super::{ApiError, ApiJson, ApiQuery};
use provider_protocol_test::run_protocol_test;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/providers", get(list).post(create))
        .route("/providers/discover-models", post(discover_models))
        .route("/providers/test-protocol", post(test_protocol))
        .route("/providers/:provider_id/ping", post(ping))
        .route(
            "/providers/:provider_id/test-protocol",
            post(test_protocol_for_provider),
        )
        .route(
            "/providers/:provider_id/sharing",
            get(get_sharing).patch(update_sharing),
        )
        .route("/providers/:provider_id/usage", get(get_usage))
        .route(
            "/providers/:provider_id",
            get(get_one).patch(update).delete(delete_one),
        )
}

#[derive(Deserialize)]
struct ProviderUsageQuery {
    range: Option<StatsRange>,
}

async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
) -> Result<Json<Vec<ProviderListItemResponse>>, ApiError> {
    let providers = state
        .storage
        .list_visible_providers(&identity.id, Some(&state.provider_catalog))
        .await?;
    Ok(Json(providers))
}

async fn create(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    ApiJson(input): ApiJson<CreateProviderRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), ApiError> {
    let provider_id = state
        .storage
        .create_provider_with_catalog(&identity.id, input, &state.provider_catalog)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .storage
                .get_provider_with_catalog(&identity.id, &provider_id, &state.provider_catalog)
                .await?,
        ),
    ))
}

async fn get_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<Json<ProviderResponse>, ApiError> {
    Ok(Json(
        state
            .storage
            .get_provider_with_catalog(&identity.id, &provider_id, &state.provider_catalog)
            .await?,
    ))
}

async fn update(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
    ApiJson(input): ApiJson<UpdateProviderRequest>,
) -> Result<Json<ProviderResponse>, ApiError> {
    Ok(Json(
        state
            .storage
            .update_provider_with_catalog(
                &identity.id,
                &provider_id,
                input,
                &state.provider_catalog,
            )
            .await?,
    ))
}

async fn delete_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .storage
        .delete_provider(&identity.id, &provider_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ping(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<Json<ProviderOperationResponse>, ApiError> {
    let provider = state
        .storage
        .get_visible_provider(&identity.id, &provider_id)
        .await?;
    let started_at = std::time::Instant::now();
    let response = state
        .provider_client(&provider.provider.provider_type)?
        .head(&provider.provider.base_url)
        .send()
        .await;
    let latency_ms = Some(started_at.elapsed().as_millis() as i64);

    Ok(Json(match response {
        Ok(_) => ProviderOperationResponse {
            ok: true,
            protocol: None,
            latency_ms,
            first_token_ms: None,
            error: None,
            models: None,
        },
        Err(error) => ProviderOperationResponse {
            ok: false,
            protocol: None,
            latency_ms,
            first_token_ms: None,
            error: Some(if error.is_timeout() {
                "上游连接超时".to_string()
            } else {
                "上游连接失败".to_string()
            }),
            models: None,
        },
    }))
}

async fn get_sharing(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<Json<prelay_protocol::ProviderSharingResponse>, ApiError> {
    Ok(Json(
        state
            .storage
            .get_provider_sharing(&identity.id, &provider_id)
            .await?,
    ))
}

async fn update_sharing(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
    ApiJson(input): ApiJson<prelay_protocol::UpdateProviderSharingRequest>,
) -> Result<Json<prelay_protocol::ProviderSharingResponse>, ApiError> {
    Ok(Json(
        state
            .storage
            .update_provider_sharing(&identity.id, &provider_id, input)
            .await?,
    ))
}

async fn get_usage(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
    ApiQuery(query): ApiQuery<ProviderUsageQuery>,
) -> Result<Json<ProviderUsageResponse>, ApiError> {
    Ok(Json(
        state
            .storage
            .get_visible_provider_usage(&identity.id, &provider_id, query.range.unwrap_or_default())
            .await?,
    ))
}

async fn discover_models(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<ProviderOperationRequest>,
) -> Result<Json<ProviderOperationResponse>, ApiError> {
    let models = match model_discovery::discover_models(
        &state.provider_client(&input.provider_type)?,
        &input.provider_type,
        &input.base_url,
        &input.api_key,
    )
    .await
    {
        Ok(models) => models,
        Err(error) => {
            return Ok(Json(ProviderOperationResponse {
                ok: false,
                protocol: None,
                latency_ms: None,
                first_token_ms: None,
                error: Some(error.public_message()),
                models: None,
            }));
        }
    };
    Ok(Json(ProviderOperationResponse {
        ok: true,
        protocol: None,
        latency_ms: None,
        first_token_ms: None,
        error: None,
        models: Some(models),
    }))
}

async fn test_protocol(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<ProviderOperationRequest>,
) -> Result<Json<ProviderOperationResponse>, ApiError> {
    Ok(Json(
        run_protocol_test(
            &state.provider_client(&input.provider_type)?,
            &input.provider_type,
            input.protocol.as_deref(),
            &input.base_url,
            &input.api_key,
            input.model.as_deref(),
        )
        .await?,
    ))
}

async fn test_protocol_for_provider(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
    ApiJson(input): ApiJson<TestProviderProtocolRequest>,
) -> Result<Json<ProviderOperationResponse>, ApiError> {
    let provider = state
        .storage
        .get_visible_provider(&identity.id, &provider_id)
        .await?;
    if !provider.can_manage {
        return Err(crate::storage::StorageError::ProviderSharingNotAllowed.into());
    }
    let api_key = state
        .storage
        .decrypt_provider_key(&identity.id, &provider_id)
        .await?;
    Ok(Json(
        run_protocol_test(
            &state.provider_client(&provider.provider.provider_type)?,
            &provider.provider.provider_type,
            Some(&input.protocol),
            &provider.provider.base_url,
            &api_key,
            input.model.as_deref(),
        )
        .await?,
    ))
}

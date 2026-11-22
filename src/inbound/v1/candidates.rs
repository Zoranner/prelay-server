use std::{future::Future, time::Duration};

use axum::{body::Body, response::Response};
use futures::StreamExt;

use crate::{
    error::AppError, inbound::v1::endpoint_resolver::ResolvedEndpointProvider,
    outbound::client::retry_with_policy, service::failover::FailoverRegistry,
    service::quota::ReservationGuard,
};

pub(super) async fn run_endpoint_model_candidates<T, F, Fut, R, RFut>(
    candidates: Vec<ResolvedEndpointProvider>,
    failover: &FailoverRegistry,
    no_candidate_error: AppError,
    mut reserve: R,
    mut attempt: F,
) -> Result<(T, String, ReservationGuard), AppError>
where
    F: FnMut(ResolvedEndpointProvider) -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
    R: FnMut(&ResolvedEndpointProvider) -> RFut,
    RFut: Future<Output = Result<ReservationGuard, AppError>>,
{
    let policy = crate::outbound::client::policy();
    let mut last_upstream_error = None;

    for resolved in order_candidates(candidates, failover)
        .into_iter()
        .take(policy.max_candidates.unwrap_or(usize::MAX))
    {
        let provider_id = resolved.provider.id.clone();
        let guard = match reserve(&resolved).await {
            Ok(guard) => guard,
            Err(error) => {
                failover.rest(&provider_id, Duration::from_secs(30));
                last_upstream_error = Some(error);
                continue;
            }
        };
        match retry_with_policy(policy, || attempt(resolved.clone())).await {
            Ok(response) => {
                failover.clear(&provider_id);
                return Ok((response, provider_id, guard));
            }
            Err(error) if error.is_failoverable_upstream() => {
                drop(guard);
                if let Some(base) = crate::service::failover::rest_base(&error) {
                    failover.rest(&provider_id, base);
                }
                last_upstream_error = Some(error);
            }
            Err(error) => {
                drop(guard);
                return Err(error);
            }
        }
    }

    Err(last_upstream_error.unwrap_or(no_candidate_error))
}

/// 把预留绑定到响应体生命周期，流式响应结束或断开后才释放。
pub(crate) fn hold_reservation(
    response: Response,
    mut guard: ReservationGuard,
    settle_pending: bool,
) -> Response {
    guard.set_settle_on_drop(settle_pending && response.status().is_success());
    let (parts, body) = response.into_parts();
    let body = Body::from_stream(body.into_data_stream().map(move |chunk| {
        let _held = &guard;
        chunk
    }));
    Response::from_parts(parts, body)
}

/// 冷却中的候选排到最后（各自保持原有相对顺序），但不会被丢弃。
fn order_candidates(
    candidates: Vec<ResolvedEndpointProvider>,
    failover: &FailoverRegistry,
) -> Vec<ResolvedEndpointProvider> {
    let (resting, ready): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|candidate| failover.resting(&candidate.provider.id));
    ready.into_iter().chain(resting).collect()
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, response::Response};
    use futures::stream;

    use super::{hold_reservation, run_endpoint_model_candidates};
    use crate::{
        codec::providers::spec::UpstreamProtocol,
        domain::registry::ProviderConfig,
        error::AppError,
        inbound::v1::endpoint_resolver::ResolvedEndpointProvider,
        service::{failover::FailoverRegistry, quota::QuotaReservations},
    };
    use axum::http::StatusCode;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn provider(id: &str) -> ResolvedEndpointProvider {
        ResolvedEndpointProvider {
            provider: ProviderConfig {
                id: id.to_string(),
                name: format!("provider {id}"),
                provider_type: "test".to_string(),
                base_url: "http://127.0.0.1:1".to_string(),
                api_key: String::new(),
                capabilities_json: None,
                models: Vec::new(),
                created_at: String::new(),
            },
            model_upstream: "test-model".to_string(),
            upstream_protocol: UpstreamProtocol::ChatCompletions,
        }
    }

    fn upstream_error(status: u16) -> AppError {
        AppError::Upstream {
            status: Some(StatusCode::from_u16(status).expect("valid status")),
            message: String::new(),
        }
    }

    #[tokio::test]
    async fn resting_candidates_are_tried_last() {
        let registry = FailoverRegistry::default();
        registry.rest("a", Duration::from_secs(60));
        let tried = Arc::new(Mutex::new(Vec::new()));
        let seen = tried.clone();

        let result = run_endpoint_model_candidates(
            vec![provider("a"), provider("b")],
            &registry,
            AppError::BadRequest("no candidate".to_string()),
            |_| async { Ok::<_, AppError>(crate::service::quota::ReservationGuard::none()) },
            move |candidate| {
                let seen = seen.clone();
                async move {
                    let id = candidate.provider.id;
                    seen.lock().expect("tried").push(id.clone());
                    if id == "b" {
                        Err(upstream_error(502))
                    } else {
                        Ok("ok")
                    }
                }
            },
        )
        .await;

        assert_eq!(result.expect("resting candidate answers last").1, "a");
        assert_eq!(*tried.lock().expect("tried"), vec!["b", "a"]);
    }

    #[tokio::test]
    async fn failing_candidate_is_rested_and_successor_clears_its_rest() {
        let registry = FailoverRegistry::default();
        registry.rest("b", Duration::from_secs(60));

        let result = run_endpoint_model_candidates(
            vec![provider("a"), provider("b")],
            &registry,
            AppError::BadRequest("no candidate".to_string()),
            |_| async { Ok::<_, AppError>(crate::service::quota::ReservationGuard::none()) },
            |candidate| async move {
                if candidate.provider.id == "a" {
                    Err(upstream_error(502))
                } else {
                    Ok("ok")
                }
            },
        )
        .await;

        assert_eq!(result.expect("second candidate answers").1, "b");
        assert!(registry.resting("a"), "failed candidate should be rested");
        assert!(
            !registry.resting("b"),
            "answering candidate should be cleared"
        );
    }

    #[tokio::test]
    async fn single_resting_candidate_is_still_tried() {
        let registry = FailoverRegistry::default();
        registry.rest("a", Duration::from_secs(60));

        let result = run_endpoint_model_candidates(
            vec![provider("a")],
            &registry,
            AppError::BadRequest("no candidate".to_string()),
            |_| async { Ok::<_, AppError>(crate::service::quota::ReservationGuard::none()) },
            |_candidate| async move { Ok::<_, AppError>("ok") },
        )
        .await;

        assert_eq!(result.expect("lone candidate answers").1, "a");
    }

    #[tokio::test]
    async fn non_failoverable_error_stops_the_loop() {
        let registry = FailoverRegistry::default();
        let tried = Arc::new(Mutex::new(Vec::new()));
        let seen = tried.clone();

        let result = run_endpoint_model_candidates(
            vec![provider("a"), provider("b")],
            &registry,
            AppError::BadRequest("no candidate".to_string()),
            |_| async { Ok::<_, AppError>(crate::service::quota::ReservationGuard::none()) },
            move |candidate| {
                let seen = seen.clone();
                async move {
                    seen.lock().expect("tried").push(candidate.provider.id);
                    Err::<&str, _>(AppError::BadRequest("bad request".to_string()))
                }
            },
        )
        .await;

        assert!(result.is_err());
        assert_eq!(*tried.lock().expect("tried"), vec!["a"]);
    }

    #[tokio::test]
    async fn reservation_is_held_until_the_response_is_dropped() {
        let reservations = QuotaReservations::default();
        let windows = vec![prelay_protocol::QuotaWindow {
            kind: prelay_protocol::QuotaWindowKind::Day,
            span_secs: Some(24 * 60 * 60),
            used_percent: 0.0,
            resets_at: Some("2026-11-18T00:00:00+08:00".to_string()),
            remaining: Some(1000.0),
            limit: Some(1000.0),
            unit: Some(prelay_protocol::QuotaUnit::Tokens),
            display: None,
        }];
        let guard = reservations
            .reserve_windows("provider", "identity", &windows, 100)
            .expect("reserve");
        assert_eq!(reservations.held_total("provider", "identity"), 100);

        let response = hold_reservation(
            Response::new(Body::from_stream(stream::pending::<
                Result<bytes::Bytes, std::convert::Infallible>,
            >())),
            guard,
            false,
        );
        assert_eq!(reservations.held_total("provider", "identity"), 100);
        drop(response);
        assert_eq!(reservations.held_total("provider", "identity"), 0);
    }
}

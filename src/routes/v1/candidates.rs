use std::future::Future;

use crate::{
    error::AppError, failover::FailoverRegistry,
    routes::v1::endpoint_resolver::ResolvedEndpointProvider, upstream::retry_with_policy,
};

pub(super) async fn run_endpoint_model_candidates<T, F, Fut>(
    candidates: Vec<ResolvedEndpointProvider>,
    failover: &FailoverRegistry,
    no_candidate_error: AppError,
    mut attempt: F,
) -> Result<(T, String), AppError>
where
    F: FnMut(ResolvedEndpointProvider) -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    let policy = crate::upstream::policy();
    let mut last_upstream_error = None;

    for resolved in order_candidates(candidates, failover)
        .into_iter()
        .take(policy.max_candidates.unwrap_or(usize::MAX))
    {
        let provider_id = resolved.provider.id.clone();
        match retry_with_policy(policy, || attempt(resolved.clone())).await {
            Ok(response) => {
                failover.clear(&provider_id);
                return Ok((response, provider_id));
            }
            Err(error) if error.is_failoverable_upstream() => {
                if let Some(base) = crate::failover::rest_base(&error) {
                    failover.rest(&provider_id, base);
                }
                last_upstream_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_upstream_error.unwrap_or(no_candidate_error))
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
    use super::run_endpoint_model_candidates;
    use crate::{
        error::AppError, failover::FailoverRegistry, models::ProviderConfig,
        providers::spec::UpstreamProtocol, routes::v1::endpoint_resolver::ResolvedEndpointProvider,
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
                token: String::new(),
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
}

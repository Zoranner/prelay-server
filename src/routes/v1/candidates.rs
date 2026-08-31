use std::future::Future;

use crate::{
    error::AppError, routes::v1::endpoint_resolver::ResolvedEndpointProvider,
    upstream::retry_with_policy,
};

pub(super) async fn run_endpoint_model_candidates<T, F, Fut>(
    candidates: Vec<ResolvedEndpointProvider>,
    no_candidate_error: AppError,
    mut attempt: F,
) -> Result<(T, String), AppError>
where
    F: FnMut(ResolvedEndpointProvider) -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    let policy = crate::upstream::policy();
    let mut last_upstream_error = None;

    for resolved in candidates
        .into_iter()
        .take(policy.max_candidates.unwrap_or(usize::MAX))
    {
        let provider_id = resolved.provider.id.clone();
        match retry_with_policy(policy, || attempt(resolved.clone())).await {
            Ok(response) => return Ok((response, provider_id)),
            Err(error) if error.is_failoverable_upstream() => last_upstream_error = Some(error),
            Err(error) => return Err(error),
        }
    }

    Err(last_upstream_error.unwrap_or(no_candidate_error))
}

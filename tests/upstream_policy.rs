use std::{
    sync::{atomic::AtomicUsize, Arc},
    time::Duration,
};

use axum::http::StatusCode;
use prelay_server::{
    error::AppError,
    upstream::{retry_with_policy, UpstreamPolicy},
};

#[test]
fn uses_safe_defaults_when_optional_environment_variables_are_missing() {
    let policy = UpstreamPolicy::from_values(None, None, None, None).expect("default policy");

    assert_eq!(policy.timeout, Duration::from_secs(300));
    assert_eq!(policy.max_retries, 0);
    assert_eq!(policy.retry_backoff, Duration::from_millis(250));
    assert_eq!(policy.max_candidates, None);
}

#[test]
fn parses_global_upstream_policy_values() {
    let policy = UpstreamPolicy::from_values(Some("45"), Some("2"), Some("500"), Some("3"))
        .expect("valid policy");

    assert_eq!(policy.timeout, Duration::from_secs(45));
    assert_eq!(policy.max_retries, 2);
    assert_eq!(policy.retry_backoff, Duration::from_millis(500));
    assert_eq!(policy.max_candidates, Some(3));
}

#[test]
fn rejects_zero_timeout_and_candidate_limit() {
    assert!(UpstreamPolicy::from_values(Some("0"), None, None, None).is_err());
    assert!(UpstreamPolicy::from_values(None, None, None, Some("0")).is_err());
}

#[tokio::test]
async fn retries_a_retryable_candidate_until_it_succeeds() {
    let policy =
        UpstreamPolicy::from_values(None, Some("2"), Some("0"), None).expect("valid retry policy");
    let attempts = Arc::new(AtomicUsize::new(0));

    let response = retry_with_policy(&policy, || {
        let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async move {
            if attempt < 2 {
                Err(AppError::Upstream {
                    status: Some(StatusCode::SERVICE_UNAVAILABLE),
                    message: "temporary upstream failure".to_string(),
                })
            } else {
                Ok("healthy response")
            }
        }
    })
    .await
    .expect("retry should reach the healthy response");

    assert_eq!(response, "healthy response");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
}

use std::{future::Future, sync::OnceLock, time::Duration};

use crate::error::AppError;

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const DEFAULT_RETRY_BACKOFF_MS: u64 = 250;

static UPSTREAM_POLICY: OnceLock<UpstreamPolicy> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamPolicy {
    pub timeout: Duration,
    pub max_retries: usize,
    pub retry_backoff: Duration,
    pub max_candidates: Option<usize>,
}

impl Default for UpstreamPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_retries: 0,
            retry_backoff: Duration::from_millis(DEFAULT_RETRY_BACKOFF_MS),
            max_candidates: None,
        }
    }
}

impl UpstreamPolicy {
    pub fn from_environment() -> Result<Self, String> {
        Self::from_values(
            std::env::var("UPSTREAM_TIMEOUT_SECS").ok().as_deref(),
            std::env::var("UPSTREAM_MAX_RETRIES").ok().as_deref(),
            std::env::var("UPSTREAM_RETRY_BACKOFF_MS").ok().as_deref(),
            std::env::var("UPSTREAM_MAX_CANDIDATES").ok().as_deref(),
        )
    }

    pub fn from_values(
        timeout_secs: Option<&str>,
        max_retries: Option<&str>,
        retry_backoff_ms: Option<&str>,
        max_candidates: Option<&str>,
    ) -> Result<Self, String> {
        let timeout_secs =
            parse_positive("UPSTREAM_TIMEOUT_SECS", timeout_secs)?.unwrap_or(DEFAULT_TIMEOUT_SECS);
        let max_retries = parse_usize("UPSTREAM_MAX_RETRIES", max_retries)?.unwrap_or(0);
        let retry_backoff_ms = parse_usize("UPSTREAM_RETRY_BACKOFF_MS", retry_backoff_ms)?
            .unwrap_or(DEFAULT_RETRY_BACKOFF_MS as usize);
        let max_candidates =
            parse_positive("UPSTREAM_MAX_CANDIDATES", max_candidates)?.map(|value| value as usize);

        Ok(Self {
            timeout: Duration::from_secs(timeout_secs),
            max_retries,
            retry_backoff: Duration::from_millis(retry_backoff_ms as u64),
            max_candidates,
        })
    }
}

pub fn initialize_from_environment() -> Result<&'static UpstreamPolicy, String> {
    let upstream_policy = UpstreamPolicy::from_environment()?;
    UPSTREAM_POLICY
        .set(upstream_policy)
        .map_err(|_| "upstream policy was already initialized".to_owned())?;
    Ok(policy())
}

pub fn policy() -> &'static UpstreamPolicy {
    UPSTREAM_POLICY.get_or_init(UpstreamPolicy::default)
}

pub async fn retry_with_policy<T, F, Fut>(
    policy: &UpstreamPolicy,
    mut request: F,
) -> Result<T, AppError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    for attempt in 0..=policy.max_retries {
        match request().await {
            Ok(response) => return Ok(response),
            Err(error) if error.is_retryable_upstream() && attempt < policy.max_retries => {
                tokio::time::sleep(policy.retry_backoff).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the retry loop always returns a response or error")
}

fn parse_positive(name: &str, value: Option<&str>) -> Result<Option<u64>, String> {
    match value {
        Some(value) => match value.parse::<u64>() {
            Ok(value) if value > 0 => Ok(Some(value)),
            _ => Err(format!("{name} must be a positive integer")),
        },
        None => Ok(None),
    }
}

fn parse_usize(name: &str, value: Option<&str>) -> Result<Option<usize>, String> {
    match value {
        Some(value) => value
            .parse::<usize>()
            .map(Some)
            .map_err(|_| format!("{name} must be a non-negative integer")),
        None => Ok(None),
    }
}

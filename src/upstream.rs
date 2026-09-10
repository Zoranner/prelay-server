use std::{future::Future, sync::OnceLock, time::Duration};

use crate::error::AppError;

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_RETRY_BACKOFF_MS: u64 = 250;

static UPSTREAM_POLICY: OnceLock<UpstreamPolicy> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamPolicy {
    /// Timeout for establishing an upstream connection, including the TLS handshake.
    pub connect_timeout: Duration,
    /// Idle read timeout for upstream responses; every received chunk restarts it.
    /// It also bounds the wait for the response head.
    pub read_timeout: Duration,
    pub max_retries: usize,
    pub retry_backoff: Duration,
    pub max_candidates: Option<usize>,
}

impl Default for UpstreamPolicy {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
            read_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_retries: 0,
            retry_backoff: Duration::from_millis(DEFAULT_RETRY_BACKOFF_MS),
            max_candidates: None,
        }
    }
}

impl UpstreamPolicy {
    pub fn from_environment() -> Result<Self, String> {
        Self::from_values(
            std::env::var("UPSTREAM_CONNECT_TIMEOUT_SECS")
                .ok()
                .as_deref(),
            std::env::var("UPSTREAM_TIMEOUT_SECS").ok().as_deref(),
            std::env::var("UPSTREAM_MAX_RETRIES").ok().as_deref(),
            std::env::var("UPSTREAM_RETRY_BACKOFF_MS").ok().as_deref(),
            std::env::var("UPSTREAM_MAX_CANDIDATES").ok().as_deref(),
        )
    }

    pub fn from_values(
        connect_timeout_secs: Option<&str>,
        read_timeout_secs: Option<&str>,
        max_retries: Option<&str>,
        retry_backoff_ms: Option<&str>,
        max_candidates: Option<&str>,
    ) -> Result<Self, String> {
        let connect_timeout_secs =
            parse_positive("UPSTREAM_CONNECT_TIMEOUT_SECS", connect_timeout_secs)?
                .unwrap_or(DEFAULT_CONNECT_TIMEOUT_SECS);
        let read_timeout_secs = parse_positive("UPSTREAM_TIMEOUT_SECS", read_timeout_secs)?
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        let max_retries = parse_usize("UPSTREAM_MAX_RETRIES", max_retries)?.unwrap_or(0);
        let retry_backoff_ms = parse_usize("UPSTREAM_RETRY_BACKOFF_MS", retry_backoff_ms)?
            .unwrap_or(DEFAULT_RETRY_BACKOFF_MS as usize);
        let max_candidates =
            parse_positive("UPSTREAM_MAX_CANDIDATES", max_candidates)?.map(|value| value as usize);

        Ok(Self {
            connect_timeout: Duration::from_secs(connect_timeout_secs),
            read_timeout: Duration::from_secs(read_timeout_secs),
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

pub fn build_client(policy: &UpstreamPolicy) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(policy.connect_timeout)
        .read_timeout(policy.read_timeout)
        .build()
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    use super::{build_client, UpstreamPolicy};

    const TEST_READ_TIMEOUT: Duration = Duration::from_millis(1000);
    const CHUNK_INTERVAL: Duration = Duration::from_millis(250);
    const CHUNK_COUNT: usize = 6;

    fn test_client() -> reqwest::Client {
        build_client(&UpstreamPolicy {
            connect_timeout: Duration::from_secs(1),
            read_timeout: TEST_READ_TIMEOUT,
            max_retries: 0,
            retry_backoff: Duration::ZERO,
            max_candidates: None,
        })
        .expect("build test upstream client")
    }

    async fn accept_request(listener: &TcpListener) -> TcpStream {
        let (mut socket, _) = listener.accept().await.expect("accept test connection");
        let mut request = [0_u8; 1024];
        let bytes_read = socket.read(&mut request).await.expect("read test request");
        assert!(bytes_read > 0, "test client must send a request");
        socket
    }

    #[tokio::test]
    async fn read_timeout_allows_a_stream_that_keeps_flowing() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");

        let server = tokio::spawn(async move {
            let mut socket = accept_request(&listener).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await
                .expect("write response header");
            for _ in 0..CHUNK_COUNT {
                socket
                    .write_all(b"5\r\nhello\r\n")
                    .await
                    .expect("write response chunk");
                tokio::time::sleep(CHUNK_INTERVAL).await;
            }
            socket
                .write_all(b"0\r\n\r\n")
                .await
                .expect("write response terminator");
        });

        let started = Instant::now();
        let response = test_client()
            .get(format!("http://{address}/"))
            .send()
            .await
            .expect("send test request");
        let body = response.text().await.expect("read streaming response body");
        let elapsed = started.elapsed();

        assert_eq!(body, "hello".repeat(CHUNK_COUNT));
        assert!(
            elapsed > TEST_READ_TIMEOUT,
            "stream must outlast the read timeout: {elapsed:?}"
        );
        server.await.expect("join test server");
    }

    #[tokio::test]
    async fn read_timeout_fails_when_a_stream_stalls() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");

        let server = tokio::spawn(async move {
            let mut socket = accept_request(&listener).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n")
                .await
                .expect("write response head");
            std::future::pending::<()>().await;
        });

        let response = test_client()
            .get(format!("http://{address}/"))
            .send()
            .await
            .expect("send test request");
        let started = Instant::now();
        let error = response.text().await.expect_err("stalled stream must fail");
        let elapsed = started.elapsed();

        assert!(
            elapsed >= TEST_READ_TIMEOUT,
            "stall must outlast the read timeout: {elapsed:?}"
        );
        assert!(error.is_timeout(), "expected a timeout error: {error:?}");
        server.abort();
    }
}

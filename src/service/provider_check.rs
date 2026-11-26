use prelay_protocol::ProviderOperationResponse;

use crate::{
    codec::providers::spec::{
        normalize_upstream_base_url, with_provider_headers, RequestHeaderContext, UpstreamProtocol,
    },
    domain::catalog::CatalogRequestHeader,
    error::AppError,
};

pub(crate) async fn run_protocol_test(
    client: &reqwest::Client,
    request_headers: &[CatalogRequestHeader],
    protocol_value: Option<&str>,
    base_url: &str,
    api_key: &str,
    model_value: Option<&str>,
) -> Result<ProviderOperationResponse, AppError> {
    let protocol = protocol_value.unwrap_or_default().trim();
    let upstream_protocol = UpstreamProtocol::from_capability_value(protocol)
        .ok_or_else(|| AppError::BadRequest("protocol is not supported".to_string()))?;
    if upstream_protocol == UpstreamProtocol::ImageGenerations {
        return Err(AppError::BadRequest(
            "protocol=images_generations does not support connectivity tests".to_string(),
        ));
    }
    let model = model_value
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| AppError::BadRequest("test model must not be empty".to_string()))?;
    let base_url = normalize_upstream_base_url(base_url);
    let started_at = std::time::Instant::now();
    let response = match send_protocol_test_request(
        client,
        request_headers,
        upstream_protocol,
        &base_url,
        api_key,
        model,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let (error_code, message) = protocol_test_error(error);
            return Ok(ProviderOperationResponse {
                ok: false,
                protocol: Some(protocol.to_string()),
                latency_ms: Some(started_at.elapsed().as_millis() as i64),
                first_token_ms: None,
                error_code: Some(error_code.to_string()),
                error: Some(message),
            });
        }
    };
    let Some(response) = response else {
        return Err(AppError::BadRequest(
            "protocol=images_generations does not support connectivity tests".to_string(),
        ));
    };
    let latency_ms = Some(started_at.elapsed().as_millis() as i64);
    if !response.status().is_success() {
        return Ok(ProviderOperationResponse {
            ok: false,
            protocol: Some(protocol.to_string()),
            latency_ms,
            first_token_ms: None,
            error_code: Some("upstream_status".to_string()),
            error: Some(format!(
                "upstream returned status={}",
                response.status().as_u16()
            )),
        });
    }
    let first_token_ms = match first_response_byte_ms(response, started_at).await {
        Ok(first_token_ms) => first_token_ms,
        Err(error) => {
            let (error_code, message) = protocol_test_error(error);
            return Ok(ProviderOperationResponse {
                ok: false,
                protocol: Some(protocol.to_string()),
                latency_ms: Some(started_at.elapsed().as_millis() as i64),
                first_token_ms: None,
                error_code: Some(error_code.to_string()),
                error: Some(message),
            });
        }
    };
    Ok(ProviderOperationResponse {
        ok: true,
        protocol: Some(protocol.to_string()),
        latency_ms,
        first_token_ms,
        error_code: None,
        error: None,
    })
}

async fn send_protocol_test_request(
    client: &reqwest::Client,
    request_headers: &[CatalogRequestHeader],
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<Option<reqwest::Response>, reqwest::Error> {
    let session_id = format!("conv_{}", uuid::Uuid::new_v4().simple());
    let context = RequestHeaderContext {
        session_id: Some(&session_id),
    };
    let request = |url: String| {
        with_provider_headers(
            client.post(url).bearer_auth(api_key),
            request_headers,
            &context,
        )
    };

    match protocol {
        UpstreamProtocol::Responses => {
            request(format!("{}/responses", base_url.trim_end_matches('/')))
                .json(&serde_json::json!({
                    "model": model,
                    "stream": true,
                    "input": [{ "role": "user", "content": "ping" }],
                    "max_output_tokens": 8
                }))
                .send()
                .await
                .map(Some)
        }
        UpstreamProtocol::ChatCompletions => request(format!(
            "{}/chat/completions",
            base_url.trim_end_matches('/')
        ))
        .json(&serde_json::json!({
            "model": model,
            "stream": true,
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 8
        }))
        .send()
        .await
        .map(Some),
        UpstreamProtocol::ImageGenerations => Ok(None),
    }
}

async fn first_response_byte_ms(
    response: reqwest::Response,
    started_at: std::time::Instant,
) -> Result<Option<i64>, reqwest::Error> {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    match stream.next().await {
        Some(Ok(_)) => Ok(Some(started_at.elapsed().as_millis() as i64)),
        Some(Err(error)) => Err(error),
        None => Ok(None),
    }
}

fn protocol_test_error(error: reqwest::Error) -> (&'static str, String) {
    if error.is_timeout() {
        return (
            "upstream_connect_timeout",
            "upstream test timed out".to_string(),
        );
    }
    if error.is_connect() {
        return (
            "upstream_connect_failed",
            "upstream connect failed".to_string(),
        );
    }
    ("upstream_test_failed", "upstream test failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::run_protocol_test;
    use crate::error::AppError;

    #[tokio::test]
    async fn rejects_images_generations_protocol_tests_without_contacting_upstream() {
        let error = run_protocol_test(
            &reqwest::Client::new(),
            &[],
            Some("images_generations"),
            "http://127.0.0.1:1",
            "test-key",
            Some("test-model"),
        )
        .await
        .expect_err("image generation protocol tests must be rejected");

        assert!(matches!(
            error,
            AppError::BadRequest(message)
                if message == "protocol=images_generations does not support connectivity tests"
        ));
    }

    #[tokio::test]
    async fn rejects_images_generations_protocol_tests_without_a_model() {
        let error = run_protocol_test(
            &reqwest::Client::new(),
            &[],
            Some("images_generations"),
            "http://127.0.0.1:1",
            "test-key",
            None,
        )
        .await
        .expect_err("image generation protocol tests must be rejected before model validation");

        assert!(matches!(
            error,
            AppError::BadRequest(message)
                if message == "protocol=images_generations does not support connectivity tests"
        ));
    }

    #[tokio::test]
    async fn applies_provider_request_headers_for_protocol_tests() {
        use axum::{
            http::{header::HeaderName, HeaderMap},
            response::IntoResponse,
            routing::post,
            Router,
        };

        use crate::domain::catalog::{CatalogRequestHeader, CatalogRequestHeaderValue};

        async fn handler(headers: HeaderMap) -> impl IntoResponse {
            let session_id = headers
                .get("x-opencode-session")
                .and_then(|value| value.to_str().ok())
                .expect("opencode session header");
            assert!(session_id.starts_with("conv_"), "{session_id}");
            (
                [("content-type", "text/event-stream")],
                "data: {\"type\":\"response.completed\"}\n\n",
            )
        }

        let app = Router::new()
            .route("/responses", post(handler))
            .route("/chat/completions", post(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream server");
        let address = listener.local_addr().expect("read upstream address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        let request_headers = [CatalogRequestHeader {
            name: HeaderName::from_static("x-opencode-session"),
            value: CatalogRequestHeaderValue::SessionId,
        }];

        for protocol in ["responses", "openai"] {
            let response = run_protocol_test(
                &reqwest::Client::new(),
                &request_headers,
                Some(protocol),
                &format!("http://{address}"),
                "test-key",
                Some("gpt-6-luna"),
            )
            .await
            .expect("run protocol test");

            assert!(response.ok, "{protocol}: {:?}", response.error);
        }
        server.abort();
    }
}

use axum::{
    body::Body,
    extract::{Extension, State},
    http::header,
    response::Response,
    routing::post,
    Json, Router,
};
use futures::TryStreamExt;
use serde_json::Value;

use crate::{
    error::AppError,
    observability::{
        stream_stats::record_first_chunk, upstream_observability::upstream_observability,
    },
    providers::spec::provider_upstream_base_url,
    routes::v1::auth::CurrentProtocolAccess,
    routes::v1::endpoint_resolver::{resolve_endpoint_model_candidates, ResolvedEndpointProvider},
    stats::RequestLogInsert,
    AppState,
};

const IMAGE_GENERATIONS_PROTOCOL: &str = "images_generations";

pub fn router() -> Router<AppState> {
    Router::new().route("/images/generations", post(create_image_generation))
}

async fn create_image_generation(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("model 不能为空".to_string()))?
        .to_string();
    let is_streaming = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let candidates =
        resolve_endpoint_model_candidates(&state, &access, &model, IMAGE_GENERATIONS_PROTOCOL)
            .await?;
    let mut last_upstream_error = None;

    for resolved in candidates.into_iter().take(
        crate::upstream::policy()
            .max_candidates
            .unwrap_or(usize::MAX),
    ) {
        let provider_id = resolved.provider.id.clone();
        match crate::upstream::retry_with_policy(crate::upstream::policy(), || {
            create_image_generation_with_candidate(
                &state,
                &access,
                payload.clone(),
                model.clone(),
                is_streaming,
                started_at,
                resolved.clone(),
            )
        })
        .await
        {
            Ok(response) => {
                if let Err(error) = state
                    .storage
                    .remember_protocol_model_provider(
                        &crate::storage::ProtocolAccess {
                            identity_id: access.identity_id.clone(),
                            endpoint_id: access.endpoint_id.clone(),
                            endpoint_name: access.endpoint_name.clone(),
                        },
                        &model,
                        &provider_id,
                    )
                    .await
                {
                    tracing::warn!(error = %error, "failed to remember active endpoint model provider");
                }
                return Ok(response);
            }
            Err(error) if error.is_retryable_upstream() => last_upstream_error = Some(error),
            Err(error) => return Err(error),
        }
    }

    Err(last_upstream_error
        .unwrap_or_else(|| AppError::BadRequest(format!("接入点未配置模型 {model}"))))
}

async fn create_image_generation_with_candidate(
    state: &AppState,
    access: &CurrentProtocolAccess,
    mut payload: Value,
    model: String,
    is_streaming: bool,
    started_at: std::time::Instant,
    resolved: ResolvedEndpointProvider,
) -> Result<Response, AppError> {
    let provider = resolved.provider;
    let model_upstream = resolved.model_upstream;

    payload["model"] = Value::String(model_upstream.clone());
    let upstream_base_url = provider_upstream_base_url(&provider, resolved.upstream_protocol);
    let upstream_url = format!(
        "{}/images/generations",
        upstream_base_url.trim_end_matches('/')
    );
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::Upstream {
            status: None,
            message: format!("上游连接失败: {error}"),
        })?;
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;
    let upstream_status = upstream_response.status();

    if !upstream_status.is_success() {
        let observability_headers = upstream_response.headers().clone();
        let error_body = upstream_response.text().await.ok();
        let observability = upstream_observability(&observability_headers, error_body.as_deref());
        state
            .storage
            .insert_request_log(
                &access.identity_id,
                image_request_log(ImageRequestLogParams {
                    access,
                    provider: &provider,
                    model_requested: model.clone(),
                    model_upstream,
                    status: "failed",
                    http_status: upstream_status.as_u16() as i64,
                    is_streaming,
                    latency_ms: started_at.elapsed().as_millis() as i64,
                    upstream_latency_ms: None,
                    upstream_request_id: observability.request_id,
                    error_message: observability.error_message,
                }),
            )
            .await?;
        return Err(AppError::Upstream {
            status: Some(upstream_status),
            message: format!("上游请求失败: {upstream_status}"),
        });
    }

    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned();
    let upstream_request_id = upstream_observability(upstream_response.headers(), None).request_id;

    if is_streaming {
        let log = image_request_log(ImageRequestLogParams {
            access,
            provider: &provider,
            model_requested: model,
            model_upstream,
            status: "success",
            http_status: upstream_status.as_u16() as i64,
            is_streaming: true,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            upstream_request_id,
            error_message: None,
        });
        let body = Body::from_stream(record_first_chunk(
            state.storage.clone(),
            access.identity_id.clone(),
            upstream_response
                .bytes_stream()
                .map_err(std::io::Error::other),
            log,
            started_at,
        ));
        return Ok(upstream_response_with_body(
            upstream_status,
            content_type,
            body,
        ));
    }

    let response_bytes = upstream_response
        .bytes()
        .await
        .map_err(|error| AppError::Upstream {
            status: None,
            message: format!("读取上游响应失败: {error}"),
        })?;
    state
        .storage
        .insert_request_log(
            &access.identity_id,
            image_request_log(ImageRequestLogParams {
                access,
                provider: &provider,
                model_requested: model,
                model_upstream,
                status: "success",
                http_status: upstream_status.as_u16() as i64,
                is_streaming: false,
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: Some(upstream_latency_ms),
                upstream_request_id,
                error_message: None,
            }),
        )
        .await?;

    Ok(upstream_response_with_body(
        upstream_status,
        content_type,
        Body::from(response_bytes),
    ))
}

struct ImageRequestLogParams<'a> {
    access: &'a CurrentProtocolAccess,
    provider: &'a crate::models::ProviderConfig,
    model_requested: String,
    model_upstream: String,
    status: &'a str,
    http_status: i64,
    is_streaming: bool,
    latency_ms: i64,
    upstream_latency_ms: Option<i64>,
    upstream_request_id: Option<String>,
    error_message: Option<String>,
}

fn image_request_log(params: ImageRequestLogParams<'_>) -> RequestLogInsert {
    RequestLogInsert {
        protocol_in: IMAGE_GENERATIONS_PROTOCOL.to_string(),
        protocol_out: IMAGE_GENERATIONS_PROTOCOL.to_string(),
        protocol_upstream: IMAGE_GENERATIONS_PROTOCOL.to_string(),
        provider_id: params.provider.id.clone(),
        provider_name: params.provider.name.clone(),
        endpoint_name: params.access.endpoint_name.clone(),
        model_requested: params.model_requested,
        model_upstream: params.model_upstream,
        status: params.status.to_string(),
        http_status: params.http_status,
        error_code: None,
        error_message: params.error_message,
        is_streaming: params.is_streaming,
        input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        latency_ms: params.latency_ms,
        upstream_latency_ms: params.upstream_latency_ms,
        first_token_ms: None,
        tool_call_count: None,
        upstream_request_id: params.upstream_request_id,
        metadata_json: None,
    }
}

fn upstream_response_with_body(
    status: reqwest::StatusCode,
    content_type: Option<reqwest::header::HeaderValue>,
    body: Body,
) -> Response {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    response
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use super::create_image_generation;
    use crate::{
        models::ProviderCapabilityOverrides,
        routes::v1::endpoint_resolver::{
            create_test_endpoint_auth, create_test_endpoint_auth_with_candidates, test_provider,
            test_provider_with_capabilities,
        },
        AppState,
    };
    use axum::{
        body::{to_bytes, Body},
        extract::State,
        http::{header, HeaderValue, Response, StatusCode},
        middleware,
        response::IntoResponse,
        routing::post,
        Json, Router,
    };
    use bytes::Bytes;
    use futures::StreamExt;
    use serde_json::{json, Value};
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct UpstreamState {
        hits: Arc<AtomicUsize>,
        payloads: Arc<Mutex<Vec<Value>>>,
        status: StatusCode,
        body: Bytes,
        content_type: HeaderValue,
        request_id: Option<HeaderValue>,
    }

    struct UpstreamFixture {
        url: String,
        hits: Arc<AtomicUsize>,
        payloads: Arc<Mutex<Vec<Value>>>,
    }

    async fn test_state() -> AppState {
        crate::test_support::test_state().await
    }

    fn image_capabilities() -> ProviderCapabilityOverrides {
        ProviderCapabilityOverrides {
            upstream_protocols: Some(vec!["images_generations".to_string()]),
            ..ProviderCapabilityOverrides::default()
        }
    }

    #[tokio::test]
    async fn forwards_only_to_image_generation_candidate_and_preserves_json_bytes() {
        let unexpected = spawn_image_upstream(
            StatusCode::OK,
            Bytes::from_static(br#"{"unexpected":true}"#),
            "application/json",
            None,
        )
        .await;
        let expected_body = Bytes::from_static(
            br#"{"created":1, "data":[{"b64_json":"aGVsbG8="},{"url":"https://images.example/private-result"}]}"#,
        );
        let image = spawn_image_upstream(
            StatusCode::OK,
            expected_body.clone(),
            "application/json; charset=utf-8",
            None,
        )
        .await;
        let state = test_state().await;
        let openai = test_provider("OpenAI only", "openai", &unexpected.url, "sk-unexpected")
            .await
            .expect("create OpenAI provider");
        let image_provider = test_provider_with_capabilities(
            "Image provider",
            "custom_image",
            &image.url,
            "sk-image",
            Some(&image_capabilities()),
        )
        .await
        .expect("create image provider");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[openai, image_provider],
            "image-public",
            "image-upstream",
        )
        .await;
        let identity_id = auth.access.0.identity_id.clone();

        let response = create_image_generation(
            State(state.clone()),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect("create image generation");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json; charset=utf-8"))
        );
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("read response body"),
            expected_body
        );
        assert_eq!(unexpected.hits.load(Ordering::SeqCst), 0);
        assert_eq!(image.hits.load(Ordering::SeqCst), 1);
        {
            let payloads = image.payloads.lock().expect("lock image payloads");
            assert_eq!(
                payloads.as_slice(),
                &[json!({
                    "model": "image-upstream",
                    "prompt": "private prompt"
                })]
            );
        }

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load image request log");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].protocol_in.as_deref(), Some("images_generations"));
        assert_eq!(
            logs[0].protocol_upstream.as_deref(),
            Some("images_generations")
        );
        assert_eq!(logs[0].input_tokens, None);
        assert_eq!(logs[0].output_tokens, None);
        let summary = serde_json::to_string(&logs[0]).expect("serialize request log summary");
        assert!(!summary.contains("private prompt"));
        assert!(!summary.contains("aGVsbG8="));
        assert!(!summary.contains("https://images.example/private-result"));
    }

    #[tokio::test]
    async fn rejects_image_generation_without_image_protocol_candidate() {
        let state = test_state().await;
        let provider = test_provider("OpenAI only", "openai", "http://127.0.0.1:1", "sk-upstream")
            .await
            .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image-public", "image-upstream")
                .await;

        let error = create_image_generation(
            State(state),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect_err("provider without image protocol must be rejected");

        assert!(format!("{error:?}").contains("images_generations"));
    }

    #[tokio::test]
    async fn streams_image_events_without_waiting_for_upstream_done() {
        let upstream = spawn_streaming_image_upstream().await;
        let state = test_state().await;
        let provider = test_provider_with_capabilities(
            "Streaming image provider",
            "custom_image",
            &upstream,
            "sk-image",
            Some(&image_capabilities()),
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image-public", "image-upstream")
                .await;
        let identity_id = auth.access.0.identity_id.clone();
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    crate::routes::v1::auth::require_protocol_auth,
                )),
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let started = std::time::Instant::now();
        let response = reqwest::Client::new()
            .post(format!("http://{address}/v1/images/generations"))
            .bearer_auth(&auth.token)
            .json(&json!({
                "model": "image-public",
                "prompt": "private prompt",
                "stream": true
            }))
            .send()
            .await
            .expect("send image request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream; charset=utf-8")
        );

        let mut stream = response.bytes_stream();
        let first = stream
            .next()
            .await
            .expect("first image event")
            .expect("first image event bytes");
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(200),
            "first image event arrived after {elapsed:?}"
        );
        assert_eq!(
            first,
            Bytes::from_static(
                b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"aGVs\"}\n\n"
            )
        );

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load streaming image log");
        assert_eq!(logs.len(), 1);
        assert!(logs[0].first_token_ms.is_some());
        let summary = serde_json::to_string(&logs[0]).expect("serialize request log summary");
        assert!(!summary.contains("private prompt"));
        assert!(!summary.contains("aGVs"));

        server.abort();
    }

    #[tokio::test]
    async fn records_rate_limit_observability_without_image_or_prompt_content() {
        let upstream = spawn_image_upstream(
            StatusCode::TOO_MANY_REQUESTS,
            Bytes::from_static(
                br#"{"error":{"message":"provider overloaded"},"prompt":"private prompt","b64_json":"secret-image"}"#,
            ),
            "application/json",
            Some("cf-ray-image-123"),
        )
        .await;
        let state = test_state().await;
        let provider = test_provider_with_capabilities(
            "Rate limited image provider",
            "custom_image",
            &upstream.url,
            "sk-image",
            Some(&image_capabilities()),
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image-public", "image-upstream")
                .await;
        let identity_id = auth.access.0.identity_id.clone();

        let error = create_image_generation(
            State(state.clone()),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect_err("rate limited image request must fail");
        assert!(format!("{error:?}").contains("429"));

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load failed image log");
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].upstream_request_id.as_deref(),
            Some("cf-ray-image-123")
        );
        assert_eq!(
            logs[0].error_message.as_deref(),
            Some("provider overloaded")
        );
        let summary = serde_json::to_string(&logs[0]).expect("serialize request log summary");
        assert!(!summary.contains("private prompt"));
        assert!(!summary.contains("secret-image"));
        assert!(!summary.contains("b64_json"));
    }

    #[tokio::test]
    async fn fails_over_from_server_error_to_second_image_candidate() {
        let primary = spawn_image_upstream(
            StatusCode::INTERNAL_SERVER_ERROR,
            Bytes::from_static(br#"{"error":{"message":"primary unavailable"}}"#),
            "application/json",
            None,
        )
        .await;
        let expected_body =
            Bytes::from_static(br#"{"data":[{"url":"https://images.example/result"}]}"#);
        let backup = spawn_image_upstream(
            StatusCode::OK,
            expected_body.clone(),
            "application/json",
            None,
        )
        .await;
        let state = test_state().await;
        let primary_provider = test_provider_with_capabilities(
            "Primary image provider",
            "custom_image",
            &primary.url,
            "sk-primary",
            Some(&image_capabilities()),
        )
        .await
        .expect("create primary provider");
        let backup_provider = test_provider_with_capabilities(
            "Backup image provider",
            "custom_image",
            &backup.url,
            "sk-backup",
            Some(&image_capabilities()),
        )
        .await
        .expect("create backup provider");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[primary_provider, backup_provider],
            "image-public",
            "image-upstream",
        )
        .await;
        let identity_id = auth.access.0.identity_id.clone();

        let response = create_image_generation(
            State(state.clone()),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect("fall back to backup image provider");

        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("read backup response"),
            expected_body
        );
        assert_eq!(primary.hits.load(Ordering::SeqCst), 1);
        assert_eq!(backup.hits.load(Ordering::SeqCst), 1);
        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load failover logs");
        assert_eq!(logs.len(), 2);
        assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
        assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 1);
    }

    async fn spawn_image_upstream(
        status: StatusCode,
        body: Bytes,
        content_type: &'static str,
        request_id: Option<&'static str>,
    ) -> UpstreamFixture {
        async fn handler(
            State(state): State<UpstreamState>,
            Json(payload): Json<Value>,
        ) -> Response<Body> {
            state.hits.fetch_add(1, Ordering::SeqCst);
            state
                .payloads
                .lock()
                .expect("lock upstream payloads")
                .push(payload);
            let mut response = Response::new(Body::from(state.body));
            *response.status_mut() = state.status;
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, state.content_type);
            if let Some(request_id) = state.request_id {
                response.headers_mut().insert("cf-ray", request_id);
            }
            response
        }

        let hits = Arc::new(AtomicUsize::new(0));
        let payloads = Arc::new(Mutex::new(Vec::new()));
        let state = UpstreamState {
            hits: Arc::clone(&hits),
            payloads: Arc::clone(&payloads),
            status,
            body,
            content_type: HeaderValue::from_static(content_type),
            request_id: request_id.map(HeaderValue::from_static),
        };
        let app = Router::new()
            .route("/images/generations", post(handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind image upstream");
        let address = listener.local_addr().expect("image upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve image upstream");
        });
        UpstreamFixture {
            url: format!("http://{address}"),
            hits,
            payloads,
        }
    }

    async fn spawn_streaming_image_upstream() -> String {
        async fn handler(Json(payload): Json<Value>) -> axum::response::Response {
            assert_eq!(payload["model"], "image-upstream");
            assert_eq!(payload["prompt"], "private prompt");
            assert_eq!(payload["stream"], true);
            let stream = futures::stream::unfold(0, |step| async move {
                match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"aGVs\"}\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, Infallible>(Bytes::from_static(
                                b"data: {\"type\":\"image_generation.completed\"}\n\n",
                            )),
                            2,
                        ))
                    }
                    _ => None,
                }
            });
            (
                [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
                Body::from_stream(stream),
            )
                .into_response()
        }

        let app = Router::new().route("/images/generations", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind streaming image upstream");
        let address = listener.local_addr().expect("image upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve streaming image upstream");
        });
        format!("http://{address}")
    }
}

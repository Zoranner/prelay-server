    #[tokio::test]
    async fn logs_sanitized_failure_when_non_streaming_body_is_interrupted() {
        let upstream_url = spawn_interrupted_non_streaming_image_upstream().await;
        let state = test_state().await;
        let provider = test_provider_with_capabilities(
            "Image provider",
            "custom_image",
            &upstream_url,
            "sk-private-provider-key",
            Some(&image_capabilities()),
        )
        .await
        .expect("create image provider");
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
        .expect_err("interrupted upstream body must fail");

        match error {
            crate::error::AppError::Upstream { status, message } => {
                assert_eq!(status, None);
                assert_eq!(message, "读取上游响应失败");
            }
            other => panic!("expected upstream error, got {other:?}"),
        }
        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load body failure request log");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].protocol_in.as_deref(), Some("images_generations"));
        assert_eq!(logs[0].status, "failed");
        assert_eq!(logs[0].error_code.as_deref(), Some("upstream_body"));
        assert_eq!(logs[0].error_message.as_deref(), Some("读取上游响应失败"));
        let summary = serde_json::to_string(&logs[0]).expect("serialize request log summary");
        for secret in [
            upstream_url.as_str(),
            "private prompt",
            auth.token.as_str(),
            "sk-private-provider-key",
            "private-image-base64",
            "https://images.example/private-result",
        ] {
            assert!(!summary.contains(secret));
        }
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

    async fn spawn_connection_failure_upstream() -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind connection failure upstream");
        let address = listener
            .local_addr()
            .expect("connection failure upstream address");
        tokio::spawn(async move {
            loop {
                let (connection, _) = listener
                    .accept()
                    .await
                    .expect("accept connection failure request");
                drop(connection);
            }
        });
        format!("http://{address}")
    }

    async fn spawn_interrupted_non_streaming_image_upstream() -> String {
        async fn handler(Json(payload): Json<Value>) -> axum::response::Response {
            assert_eq!(payload["model"], "image-upstream");
            assert_eq!(payload["prompt"], "private prompt");
            let stream = futures::stream::unfold(0, |step| async move {
                match step {
                    0 => Some((
                        Ok::<_, io::Error>(Bytes::from_static(
                            br#"{"data":[{"b64_json":"private-image-base64","url":"https://images.example/private-result"}"#,
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Some((Err(io::Error::other("forced body interruption")), 2))
                    }
                    _ => None,
                }
            });
            (
                [(header::CONTENT_TYPE, "application/json")],
                Body::from_stream(stream),
            )
                .into_response()
        }

        let app = Router::new().route("/images/generations", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind interrupted non-streaming image upstream");
        let address = listener
            .local_addr()
            .expect("interrupted non-streaming upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve interrupted non-streaming image upstream");
        });
        format!("http://{address}")
    }

    async fn spawn_streaming_image_upstream() -> (String, Arc<Notify>) {
        async fn handler(
            State(release_second_event): State<Arc<Notify>>,
            Json(payload): Json<Value>,
        ) -> axum::response::Response {
            assert_eq!(payload["model"], "image-upstream");
            assert_eq!(payload["prompt"], "private prompt");
            assert_eq!(payload["stream"], true);
            assert_eq!(payload["partial_images"], 2);
            let stream = futures::stream::unfold(
                (0, release_second_event),
                |(step, release_second_event)| async move {
                    match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"aGVs\"}\n\n",
                        )),
                        (1, release_second_event),
                    )),
                    1 => {
                        release_second_event.notified().await;
                        Some((
                            Ok::<_, Infallible>(Bytes::from_static(
                                b"data: {\"type\":\"image_generation.completed\"}\n\n",
                            )),
                            (2, release_second_event),
                        ))
                    }
                    _ => None,
                }
                },
            );
            (
                [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
                Body::from_stream(stream),
            )
                .into_response()
        }

        let release_second_event = Arc::new(Notify::new());
        let app = Router::new()
            .route("/images/generations", post(handler))
            .with_state(Arc::clone(&release_second_event));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind streaming image upstream");
        let address = listener.local_addr().expect("image upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve streaming image upstream");
        });
        (format!("http://{address}"), release_second_event)
    }

    async fn spawn_interrupted_image_upstream() -> String {
        async fn handler(Json(payload): Json<Value>) -> axum::response::Response {
            assert_eq!(payload["model"], "image-upstream");
            assert_eq!(payload["prompt"], "private prompt");
            assert_eq!(payload["stream"], true);
            assert_eq!(payload["partial_images"], 2);
            let stream = futures::stream::unfold(0, |step| async move {
                match step {
                    0 => Some((
                        Ok::<_, io::Error>(Bytes::from_static(
                            b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"aGVs\"}\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        Some((
                            Err(io::Error::other("upstream image stream interrupted")),
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
            .expect("bind interrupted image upstream");
        let address = listener.local_addr().expect("image upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve interrupted image upstream");
        });
        format!("http://{address}")
    }

    async fn spawn_image_relay(state: AppState) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state,
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
        (format!("http://{address}"), server)
    }

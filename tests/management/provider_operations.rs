use axum::{
    http::{HeaderMap, StatusCode},
    routing::{get, head, post},
    Json, Router,
};
use prelay_server::{app, test_support::test_state};

use crate::{auth::register, http::request_json};

async fn spawn_provider_actions_upstream(expected_api_key: &'static str) -> String {
    async fn models(headers: HeaderMap) -> Json<serde_json::Value> {
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk-provider-action-secret")
        );
        Json(serde_json::json!({
            "data": [{ "id": "discovered-model" }]
        }))
    }

    async fn chat(headers: HeaderMap) -> (StatusCode, &'static str) {
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk-provider-action-secret")
        );
        (StatusCode::OK, "data: {\"choices\":[]}\n\n")
    }

    assert_eq!(expected_api_key, "sk-provider-action-secret");
    let upstream = Router::new()
        .route("/models", get(models))
        .route("/chat/completions", post(chat));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let address = listener.local_addr().expect("read upstream address");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });
    format!("http://{address}")
}

#[tokio::test]
async fn management_provider_actions_do_not_persist_the_supplied_key() {
    let app = app::router(test_state().await).await.expect("build app");
    let credential = register(&app, "machine-transient", "S-1-5-21-615").await["credential"]
        .as_str()
        .expect("credential")
        .to_string();
    let base_url = spawn_provider_actions_upstream("sk-provider-action-secret").await;
    let input = serde_json::json!({
        "provider_type": "openai_compatible",
        "base_url": base_url,
        "api_key": "sk-provider-action-secret",
        "protocol": "openai",
        "model": "discovered-model"
    });

    let (status, discovered): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers/discover-models",
        Some(&credential),
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        discovered["models"],
        serde_json::json!(["discovered-model"])
    );
    assert!(!discovered.to_string().contains("sk-provider-action-secret"));

    let (status, tested): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers/test-protocol",
        Some(&credential),
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tested["ok"], true);
    assert!(!tested.to_string().contains("sk-provider-action-secret"));

    let (status, providers): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/providers", Some(&credential), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());
}

#[tokio::test]
async fn management_saved_provider_ping_checks_reachability_without_a_provider_key() {
    async fn ping(headers: HeaderMap) -> StatusCode {
        assert!(headers.get("authorization").is_none());
        StatusCode::NO_CONTENT
    }

    let upstream = Router::new().route("/", head(ping));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let address = listener.local_addr().expect("read upstream address");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });

    let app = app::router(test_state().await).await.expect("build app");
    let credential = register(&app, "machine-saved-actions", "S-1-5-21-616").await["credential"]
        .as_str()
        .expect("credential")
        .to_string();
    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(&credential),
        Some(serde_json::json!({
            "name": "Saved Provider",
            "provider_type": "openai_compatible",
            "base_url": format!("http://{address}"),
            "api_key": "sk-provider-action-secret",
            "models": ["saved-model"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, ping): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/providers/{provider_id}/ping"),
        Some(&credential),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ping["ok"], true);
    assert!(!ping.to_string().contains("sk-provider-action-secret"));
}

#[tokio::test]
async fn management_provider_operations_return_success_envelopes_for_upstream_network_failures() {
    let app = app::router(test_state().await).await.expect("build app");
    let credential = register(&app, "machine-upstream-failure", "S-1-5-21-650").await["credential"]
        .as_str()
        .expect("credential")
        .to_string();
    for (path, body, expected_error_prefixes) in [
        (
            "/api/providers/discover-models",
            serde_json::json!({
                "provider_type": "openai_compatible",
                "base_url": "http://127.0.0.1:0",
                "api_key": "sk-upstream-failure-secret"
            }),
            &["模型获取失败，上游"][..],
        ),
        (
            "/api/providers/test-protocol",
            serde_json::json!({
                "provider_type": "openai_compatible",
                "base_url": "http://127.0.0.1:0",
                "api_key": "sk-upstream-failure-secret",
                "protocol": "openai",
                "model": "unavailable-model"
            }),
            &["上游测试超时", "上游连接失败", "上游测试失败"][..],
        ),
    ] {
        let (status, response): (StatusCode, serde_json::Value) =
            request_json(&app, "POST", path, Some(&credential), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(response["ok"], false, "{path}");
        let error = response["error"].as_str().expect("operation error");
        assert!(
            expected_error_prefixes
                .iter()
                .any(|prefix| error.starts_with(prefix)),
            "{path}: {response}"
        );
    }
}

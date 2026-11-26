use axum::{
    http::{HeaderMap, StatusCode},
    routing::{head, post},
    Router,
};
use prelay_server::{bootstrap as app, test_support::test_state};

use crate::{auth::register, http::request_json};

async fn spawn_provider_actions_upstream(expected_api_key: &'static str) -> String {
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
    let upstream = Router::new().route("/chat/completions", post(chat));
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
        "provider_type": "deepseek",
        "base_url": base_url,
        "api_key": "sk-provider-action-secret",
        "protocol": "openai",
        "model": "discovered-model"
    });

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers/discover-models",
        Some(&credential),
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

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
            "provider_type": "deepseek",
            "base_url": format!("http://{address}"),
            "api_key": "sk-provider-action-secret",
            "models": ["deepseek-v4-flash"]
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
async fn management_provider_operations_return_failure_envelopes_for_upstream_network_failures() {
    let app = app::router(test_state().await).await.expect("build app");
    let credential = register(&app, "machine-upstream-failure", "S-1-5-21-650").await["credential"]
        .as_str()
        .expect("credential")
        .to_string();
    let (status, response): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers/test-protocol",
        Some(&credential),
        Some(serde_json::json!({
            "provider_type": "openai_compatible",
            "base_url": "http://127.0.0.1:0",
            "api_key": "sk-upstream-failure-secret",
            "protocol": "openai",
            "model": "unavailable-model"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["ok"], false);
    let error_code = response["error_code"].as_str().unwrap_or_default();
    assert!(
        [
            "upstream_connect_timeout",
            "upstream_connect_failed",
            "upstream_test_failed"
        ]
        .contains(&error_code),
        "{response}"
    );
    assert!(response["error"].as_str().is_some(), "{response}");
}

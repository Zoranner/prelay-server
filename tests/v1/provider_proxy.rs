use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{Request, StatusCode, Uri},
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tower::ServiceExt;

use std::path::Path;

use prelay_server::provider_catalog::ProviderCatalog;

use crate::{
    auth::register,
    test_context::{test_context_with_catalog, TestContext},
};

/// 把固定目录复制一份，并给 deepseek 条目配上代理，供代理测试使用。
fn proxy_catalog(proxy_url: &str) -> ProviderCatalog {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog");
    let target =
        std::env::temp_dir().join(format!("prelay-proxy-catalog-{}", uuid::Uuid::new_v4()));
    copy_directory(&source, &target);
    let providers_path = target.join("providers.toml");
    let providers = std::fs::read_to_string(&providers_path).expect("read fixture providers");
    let providers = providers.replace(
        "base_url = \"https://api.deepseek.example/v1\"",
        &format!("base_url = \"https://api.deepseek.example/v1\"\nproxy_url = \"{proxy_url}\""),
    );
    std::fs::write(&providers_path, providers).expect("write fixture providers");
    ProviderCatalog::load(&target).expect("load proxy catalog")
}

fn copy_directory(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).expect("create target catalog directory");
    for entry in std::fs::read_dir(source).expect("read fixture catalog directory") {
        let entry = entry.expect("read fixture entry");
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if path.is_dir() {
            copy_directory(&path, &destination);
        } else {
            std::fs::copy(&path, &destination).expect("copy fixture file");
        }
    }
}

async fn proxy_test_context(proxy_url: &str) -> TestContext {
    test_context_with_catalog(proxy_catalog(proxy_url)).await
}

async fn json_request(
    app: &axum::Router,
    method: &str,
    path: &str,
    credential: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(credential) = credential {
        builder = builder.header("authorization", format!("Bearer {credential}"));
    }
    let request = builder
        .body(Body::from(
            body.map(|body| body.to_string()).unwrap_or_default(),
        ))
        .expect("build request");
    let response = app.clone().oneshot(request).await.expect("route request");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response");
    (
        status,
        serde_json::from_slice(&body).expect("decode JSON response"),
    )
}

/// 供应商上游：记录直连次数，并把自己看到的请求地址回给测试。
async fn spawn_chat_upstream(hits: Arc<AtomicUsize>) -> String {
    async fn handler(State(hits): State<Arc<AtomicUsize>>, uri: Uri) -> axum::Json<Value> {
        hits.fetch_add(1, Ordering::SeqCst);
        axum::Json(json!({
            "id": "direct",
            "model": "deepseek-v4-pro",
            "choices": [{ "message": { "role": "assistant", "content": "direct" } }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 },
            "upstream_uri": uri.to_string()
        }))
    }

    let app = axum::Router::new().fallback(handler).with_state(hits);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let address = listener.local_addr().expect("upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    format!("http://{address}")
}

/// 简易 HTTP 代理：只回自己的应答，并记录代理收到的请求地址。
async fn spawn_fake_proxy(hits: Arc<AtomicUsize>) -> String {
    async fn handler(State(hits): State<Arc<AtomicUsize>>, uri: Uri) -> axum::Json<Value> {
        hits.fetch_add(1, Ordering::SeqCst);
        axum::Json(json!({
            "id": "proxy",
            "model": "deepseek-v4-pro",
            "choices": [{ "message": { "role": "assistant", "content": "via_proxy" } }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 },
            "upstream_uri": uri.to_string()
        }))
    }

    let app = axum::Router::new().fallback(handler).with_state(hits);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
    let address = listener.local_addr().expect("proxy address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve proxy");
    });
    format!("http://{address}")
}

#[tokio::test]
async fn proxied_provider_sends_upstream_requests_through_its_proxy() {
    let upstream_hits = Arc::new(AtomicUsize::new(0));
    let proxy_hits = Arc::new(AtomicUsize::new(0));
    let upstream_url = spawn_chat_upstream(Arc::clone(&upstream_hits)).await;
    let proxy_url = spawn_fake_proxy(Arc::clone(&proxy_hits)).await;
    let context = proxy_test_context(&proxy_url).await;
    let app = context.app;

    let identity = register(&app, "machine-proxy-forward", "S-1-5-21-proxy-forward").await;
    let credential = identity["credential"].as_str().expect("credential");

    let (status, provider) = json_request(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(json!({
            "name": "Proxied provider",
            "provider_type": "deepseek",
            "base_url": upstream_url,
            "api_key": "sk-proxied",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, endpoint) = json_request(
        &app,
        "POST",
        "/api/endpoints",
        Some(credential),
        Some(json!({
            "name": "Proxied endpoint",
            "models": [{
                "provider_id": provider["id"],
                "upstream_model": "deepseek-v4-pro"
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = endpoint["token"].as_str().expect("endpoint token");

    let (status, completion) = json_request(
        &app,
        "POST",
        "/v1/chat/completions",
        Some(token),
        Some(json!({
            "model": "deepseek-v4-pro",
            "messages": [{ "role": "user", "content": "hello" }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(completion["choices"][0]["message"]["content"], "via_proxy");
    assert!(
        completion["upstream_uri"]
            .as_str()
            .unwrap_or_default()
            .starts_with("http://127.0.0.1"),
        "代理收到的是绝对地址请求：{completion}"
    );
    assert_eq!(proxy_hits.load(Ordering::SeqCst), 1, "请求应经过代理");
    assert_eq!(
        upstream_hits.load(Ordering::SeqCst),
        0,
        "配了代理就不应直连上游"
    );
}

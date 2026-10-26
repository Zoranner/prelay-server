use prelay_protocol::{CatalogKind, ExtensionKind};
use prelay_server::extensions::{
    classify_paths, versions_from_tags, ExtensionCatalog, ExtensionCatalogConfig, GiteaTag,
};

#[test]
fn classifies_rule_plugin_and_hook_packages_by_standard_directories() {
    assert_eq!(classify_paths(&[".codex-plugin/plugin.json"]), None);
    assert_eq!(classify_paths(&[".prelay.json"]), None);
    assert_eq!(classify_paths(&["hooks/OTHER.json"]), None);
    assert_eq!(classify_paths(&["hooks/scripts/session-start.sh"]), None);

    let multi = classify_paths(&["server.json", "skills/server/SKILL.md"]).unwrap();
    assert_eq!(multi.catalog_kind, CatalogKind::Plugin);
    assert_eq!(multi.artifacts.len(), 2);
    assert_eq!(multi.artifacts[0].kind, ExtensionKind::Mcp);
    assert_eq!(multi.artifacts[1].kind, ExtensionKind::Skill);

    let skill = classify_paths(&["skills/engineering/SKILL.md", "AGENTS.md"]).unwrap();
    assert_eq!(skill.catalog_kind, CatalogKind::Plugin);
    assert_eq!(skill.artifacts.len(), 1);
    assert_eq!(skill.artifacts[0].kind, ExtensionKind::Skill);

    let rule = classify_paths(&["AGENTS.md"]).unwrap();
    assert_eq!(rule.catalog_kind, CatalogKind::Rule);
    assert_eq!(rule.artifacts.len(), 1);
    assert_eq!(rule.artifacts[0].kind, ExtensionKind::Rule);

    let hook = classify_paths(&["hooks/hooks.json"]).unwrap();
    assert_eq!(hook.catalog_kind, CatalogKind::Plugin);
    assert_eq!(hook.artifacts.len(), 1);
    assert_eq!(hook.artifacts[0].kind, ExtensionKind::Hook);
    assert_eq!(hook.artifacts[0].paths, vec!["hooks/hooks.json"]);

    let hooks = classify_paths(&[
        ".prelay.json",
        "AGENTS.md",
        "hooks/hooks.json",
        "hooks/other.json",
        "hooks/scripts/session-start.sh",
    ])
    .unwrap();
    assert_eq!(hooks.catalog_kind, CatalogKind::Plugin);
    assert_eq!(hooks.artifacts.len(), 1);
    assert_eq!(hooks.artifacts[0].kind, ExtensionKind::Hook);
    assert_eq!(
        hooks.artifacts[0].paths,
        vec![
            "hooks/hooks.json",
            "hooks/other.json",
            "hooks/scripts/session-start.sh"
        ]
    );
    assert_eq!(hooks.artifact_summaries()[0].file_count, 3);

    let unsafe_hook =
        classify_paths(&["hooks/hooks.json", "hooks/../escape.sh", "hooks//empty"]).unwrap();
    assert_eq!(unsafe_hook.artifacts.len(), 1);
    assert_eq!(unsafe_hook.artifacts[0].paths, vec!["hooks/hooks.json"]);

    assert_eq!(classify_paths(&["README.md"]), None);
}

#[test]
fn classifies_hook_artifacts_together_with_skill_and_mcp() {
    let all = classify_paths(&[
        "server.json",
        "skills/a/SKILL.md",
        "hooks/hooks.json",
        "hooks/scripts/run.sh",
    ])
    .unwrap();
    assert_eq!(all.catalog_kind, CatalogKind::Plugin);
    assert_eq!(all.artifacts.len(), 3);
    assert_eq!(all.artifacts[0].kind, ExtensionKind::Mcp);
    assert_eq!(all.artifacts[0].paths, vec!["server.json"]);
    assert_eq!(all.artifacts[1].kind, ExtensionKind::Skill);
    assert_eq!(all.artifacts[2].kind, ExtensionKind::Hook);
    assert_eq!(
        all.artifacts[2].paths,
        vec!["hooks/hooks.json", "hooks/scripts/run.sh"]
    );
}

#[test]
fn classifies_hook_with_mcp_and_with_skill() {
    let with_mcp = classify_paths(&["server.json", "hooks/hooks.json"]).unwrap();
    assert_eq!(with_mcp.artifacts.len(), 2);
    assert_eq!(with_mcp.artifacts[0].kind, ExtensionKind::Mcp);
    assert_eq!(with_mcp.artifacts[1].kind, ExtensionKind::Hook);
    assert_eq!(with_mcp.artifacts[1].paths, vec!["hooks/hooks.json"]);

    let with_skill = classify_paths(&["skills/a/SKILL.md", "hooks/hooks.json"]).unwrap();
    assert_eq!(with_skill.artifacts.len(), 2);
    assert_eq!(with_skill.artifacts[0].kind, ExtensionKind::Skill);
    assert_eq!(with_skill.artifacts[1].kind, ExtensionKind::Hook);
    assert_eq!(with_skill.artifacts[1].paths, vec!["hooks/hooks.json"]);
}

#[test]
fn keeps_only_fixed_semantic_release_tags_in_descending_order() {
    let versions = versions_from_tags(vec![
        GiteaTag::new(
            "v1.0.0",
            "a".repeat(40),
            Some("a".repeat(40)),
            Some("2026-08-27T08:30:00Z".to_string()),
        ),
        GiteaTag::new(
            "v1.2.0",
            "b".repeat(40),
            Some("b".repeat(40)),
            Some("2026-08-28T08:30:00Z".to_string()),
        ),
        GiteaTag::new(
            "main",
            "c".repeat(40),
            Some("c".repeat(40)),
            Some("2026-08-29T08:30:00Z".to_string()),
        ),
    ]);

    assert_eq!(
        versions
            .iter()
            .map(|version| version.tag.as_str())
            .collect::<Vec<_>>(),
        vec!["v1.2.0", "v1.0.0"]
    );
}

#[test]
fn extension_catalog_configuration_uses_its_own_environment_domain() {
    let config = ExtensionCatalogConfig::new(
        "https://git.example.test/",
        "agents",
        Some("read-token".to_string()),
        300,
    )
    .expect("valid extension catalog configuration");

    assert_eq!(config.gitea_url(), "https://git.example.test");
    assert_eq!(config.organization(), "agents");
    assert_eq!(config.cache_ttl().as_secs(), 300);
    assert!(ExtensionCatalogConfig::new("not-a-url", "agents", None, 300).is_err());
    assert!(
        ExtensionCatalogConfig::new("https://git.example.test", "bad/name", None, 300).is_err()
    );
    assert!(ExtensionCatalogConfig::new("https://git.example.test", "agents", None, 0).is_err());
}

#[tokio::test]
async fn builds_a_rule_install_bundle_from_the_published_commit() {
    use axum::{routing::get, Json, Router};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

    let commit_sha = "a".repeat(40);
    let app = Router::new()
        .route(
            "/api/v1/orgs/agents/repos",
            get(|| async { Json(serde_json::json!([{ "name": "engineering-rules" }])) }),
        )
        .route(
            "/api/v1/repos/agents/engineering-rules/tags",
            get({
                let commit_sha = commit_sha.clone();
                move || {
                    let commit_sha = commit_sha.clone();
                    async move {
                        Json(serde_json::json!([{
                            "name": "v1.0.0",
                            "id": commit_sha,
                            "commit": {
                                "sha": "a".repeat(40),
                                "created": "2026-08-28T08:30:00Z"
                            }
                        }]))
                    }
                }
            }),
        )
        .route(
            "/api/v1/repos/agents/engineering-rules/git/trees/:commit_sha",
            get(|| async {
                Json(serde_json::json!({
                    "tree": [{ "path": "AGENTS.md", "type": "blob" }]
                }))
            }),
        )
        .route(
            "/api/v1/repos/agents/engineering-rules/contents/AGENTS.md",
            get(|| async {
                Json(serde_json::json!({
                    "content": BASE64.encode("# Managed rules"),
                    "encoding": "base64"
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Gitea test server");
    let address = listener.local_addr().expect("read test server address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve Gitea test server");
    });
    let catalog = ExtensionCatalog::new(
        reqwest::Client::new(),
        ExtensionCatalogConfig::new(format!("http://{address}"), "agents", None, 300)
            .expect("valid Gitea test configuration"),
    );

    let bundle = catalog
        .install_bundle("engineering-rules", "v1.0.0")
        .await
        .expect("build rule install bundle");

    assert_eq!(bundle.artifacts.len(), 1);
    assert_eq!(bundle.artifacts[0].kind, ExtensionKind::Rule);
    assert_eq!(bundle.version.commit_sha, commit_sha);
    assert_eq!(bundle.artifacts[0].files.len(), 1);
    assert_eq!(bundle.artifacts[0].files[0].path, "AGENTS.md");
    assert_eq!(
        bundle.artifacts[0].files[0].content_base64,
        BASE64.encode("# Managed rules")
    );
    assert!(bundle.skipped.is_empty());
}

#[tokio::test]
async fn builds_an_mcp_install_bundle_from_a_valid_manifest() {
    use axum::{routing::get, Json, Router};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

    let commit_sha = "b".repeat(40);
    let app = Router::new()
        .route(
            "/api/v1/orgs/agents/repos",
            get(|| async { Json(serde_json::json!([{ "name": "research-mcp" }])) }),
        )
        .route(
            "/api/v1/repos/agents/research-mcp/tags",
            get({
                let commit_sha = commit_sha.clone();
                move || {
                    let commit_sha = commit_sha.clone();
                    async move {
                        Json(serde_json::json!([{
                            "name": "v1.0.0",
                            "id": commit_sha,
                            "commit": {
                                "sha": "b".repeat(40),
                                "created": "2026-08-28T08:30:00Z"
                            }
                        }]))
                    }
                }
            }),
        )
        .route(
            "/api/v1/repos/agents/research-mcp/git/trees/:commit_sha",
            get(|| async {
                Json(serde_json::json!({
                    "tree": [{ "path": "server.json", "type": "blob" }]
                }))
            }),
        )
        .route(
            "/api/v1/repos/agents/research-mcp/contents/server.json",
            get(|| async {
                Json(serde_json::json!({
                    "content": BASE64.encode(r#"{
                      "name": "research",
                      "transport": {
                        "type": "stdio",
                        "command": ["prelay-research", "--stdio"],
                        "cwd": null,
                        "environment": {},
                        "enabled": true,
                        "timeoutMs": null
                      }
                    }"#),
                    "encoding": "base64"
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Gitea test server");
    let address = listener.local_addr().expect("read test server address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve Gitea test server");
    });
    let catalog = ExtensionCatalog::new(
        reqwest::Client::new(),
        ExtensionCatalogConfig::new(format!("http://{address}"), "agents", None, 300)
            .expect("valid Gitea test configuration"),
    );

    let bundle = catalog
        .install_bundle("research-mcp", "v1.0.0")
        .await
        .expect("build MCP install bundle");

    assert_eq!(bundle.artifacts.len(), 1);
    assert_eq!(bundle.artifacts[0].kind, ExtensionKind::Mcp);
    assert_eq!(bundle.artifacts[0].files.len(), 1);
    assert_eq!(bundle.artifacts[0].files[0].path, "server.json");
}

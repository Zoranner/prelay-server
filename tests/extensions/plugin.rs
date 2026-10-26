use prelay_protocol::{CatalogKind, ExtensionKind};
use prelay_server::extensions::{CatalogError, ExtensionCatalog, ExtensionCatalogConfig};

#[tokio::test]
async fn builds_a_plugin_bundle_and_filters_catalog_by_artifact() {
    let commit_sha = "d".repeat(40);
    let base_url = spawn_catalog_server(
        "superpowers",
        commit_sha.clone(),
        vec![
            ".prelay.json",
            "AGENTS.md",
            "README.md",
            "skills/using-superpowers/SKILL.md",
            "skills/using-superpowers/notes.txt",
            "hooks/hooks.json",
            "hooks/scripts/session-start.sh",
        ],
        vec![
            ("AGENTS.md", "# development notes"),
            ("README.md", "# superpowers"),
            ("skills/using-superpowers/SKILL.md", "# using superpowers"),
            ("skills/using-superpowers/notes.txt", "notes"),
            ("hooks/hooks.json", r#"{"SessionStart": []}"#),
            (
                "hooks/scripts/session-start.sh",
                "#!/bin/sh\necho session\n",
            ),
        ],
    )
    .await;
    let catalog = ExtensionCatalog::new(
        reqwest::Client::new(),
        ExtensionCatalogConfig::new(base_url, "agents", None, 300)
            .expect("valid Gitea test configuration"),
    );

    let bundle = catalog
        .install_bundle("superpowers", "v1.0.0")
        .await
        .expect("build plugin install bundle");
    assert_eq!(bundle.artifacts.len(), 2);
    assert_eq!(bundle.artifacts[0].kind, ExtensionKind::Skill);
    assert_eq!(bundle.artifacts[0].files.len(), 2);
    assert_eq!(
        bundle.artifacts[0].files[0].path,
        "skills/using-superpowers/SKILL.md"
    );
    assert_eq!(
        bundle.artifacts[0].files[1].path,
        "skills/using-superpowers/notes.txt"
    );
    assert_eq!(bundle.artifacts[1].kind, ExtensionKind::Hook);
    assert_eq!(bundle.artifacts[1].files.len(), 2);
    assert_eq!(bundle.artifacts[1].files[0].path, "hooks/hooks.json");
    assert_eq!(
        bundle.artifacts[1].files[1].path,
        "hooks/scripts/session-start.sh"
    );
    assert_eq!(
        bundle.skipped,
        vec![
            ".prelay.json".to_string(),
            "AGENTS.md".to_string(),
            "README.md".to_string()
        ]
    );

    let plugins = catalog.list(CatalogKind::Plugin).await.unwrap();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].catalog_kind, Some(CatalogKind::Plugin));
    assert_eq!(plugins[0].artifacts.len(), 2);
    assert_eq!(plugins[0].artifacts[0].kind, ExtensionKind::Skill);
    assert_eq!(plugins[0].artifacts[0].file_count, 2);
    assert_eq!(plugins[0].artifacts[1].kind, ExtensionKind::Hook);
    assert_eq!(plugins[0].artifacts[1].file_count, 2);
    assert!(catalog.list(CatalogKind::Rule).await.unwrap().is_empty());
    assert_eq!(
        catalog
            .list_by_artifact(ExtensionKind::Skill)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(catalog
        .list_by_artifact(ExtensionKind::Mcp)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        catalog
            .list_by_artifact(ExtensionKind::Hook)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn builds_a_hook_only_bundle_and_preserves_binary_support_files() {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

    let commit_sha = "e".repeat(40);
    let binary = vec![0x00, 0x9f, 0x92, 0x96, 0xff];
    let base_url = spawn_catalog_server_bytes(
        "hook-only",
        commit_sha.clone(),
        vec![
            "hooks/scripts/run.sh",
            "hooks/lib/tool.bin",
            "hooks/hooks.json",
            "README.md",
        ],
        vec![
            ("hooks/hooks.json", br#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"sh \"${PRELAY_HOOK_ROOT}/scripts/run.sh\""}]}]}}"#.to_vec()),
            ("hooks/lib/tool.bin", binary.clone()),
            ("hooks/scripts/run.sh", b"#!/bin/sh\necho hook\n".to_vec()),
            ("README.md", b"# hook only".to_vec()),
        ],
    )
    .await;
    let catalog = ExtensionCatalog::new(
        reqwest::Client::new(),
        ExtensionCatalogConfig::new(base_url, "agents", None, 300)
            .expect("valid Gitea test configuration"),
    );

    let bundle = catalog
        .install_bundle("hook-only", "v1.0.0")
        .await
        .expect("build hook install bundle");
    assert_eq!(bundle.artifacts.len(), 1);
    assert_eq!(bundle.artifacts[0].kind, ExtensionKind::Hook);
    assert_eq!(
        bundle.artifacts[0]
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "hooks/hooks.json",
            "hooks/lib/tool.bin",
            "hooks/scripts/run.sh"
        ]
    );
    assert_eq!(
        BASE64
            .decode(&bundle.artifacts[0].files[1].content_base64)
            .unwrap(),
        binary
    );
    assert_eq!(bundle.skipped, vec!["README.md".to_string()]);

    let plugins = catalog.list(CatalogKind::Plugin).await.unwrap();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].artifacts[0].kind, ExtensionKind::Hook);
    assert_eq!(plugins[0].artifacts[0].file_count, 3);
    assert!(catalog.list(CatalogKind::Rule).await.unwrap().is_empty());
}

#[tokio::test]
async fn builds_a_hook_skill_and_mcp_bundle() {
    let commit_sha = "f".repeat(40);
    let base_url = spawn_catalog_server(
        "all-artifacts",
        commit_sha,
        vec![
            "hooks/hooks.json",
            "skills/all/SKILL.md",
            "server.json",
            "hooks/scripts/run.sh",
            "README.md",
        ],
        vec![
            (
                "hooks/hooks.json",
                r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"echo hook"}]}]}}"#,
            ),
            ("hooks/scripts/run.sh", "#!/bin/sh\n"),
            ("skills/all/SKILL.md", "# all"),
            (
                "server.json",
                r#"{"name":"all","transport":{"type":"stdio","command":["all-mcp"],"cwd":null,"environment":{},"enabled":true,"timeoutMs":null}}"#,
            ),
            ("README.md", "# all"),
        ],
    )
    .await;
    let catalog = ExtensionCatalog::new(
        reqwest::Client::new(),
        ExtensionCatalogConfig::new(base_url, "agents", None, 300)
            .expect("valid Gitea test configuration"),
    );

    let bundle = catalog
        .install_bundle("all-artifacts", "v1.0.0")
        .await
        .expect("build multi-artifact bundle");
    assert_eq!(
        bundle
            .artifacts
            .iter()
            .map(|artifact| artifact.kind)
            .collect::<Vec<_>>(),
        vec![
            ExtensionKind::Mcp,
            ExtensionKind::Skill,
            ExtensionKind::Hook
        ]
    );
    assert_eq!(bundle.artifacts[2].files.len(), 2);
    assert_eq!(bundle.skipped, vec!["README.md".to_string()]);
}

#[tokio::test]
async fn rejects_a_hook_definition_that_is_not_json() {
    let commit_sha = "1".repeat(40);
    let base_url = spawn_catalog_server(
        "broken-hook",
        commit_sha,
        vec!["hooks/hooks.json"],
        vec![("hooks/hooks.json", "{not-json")],
    )
    .await;
    let catalog = ExtensionCatalog::new(
        reqwest::Client::new(),
        ExtensionCatalogConfig::new(base_url, "agents", None, 300)
            .expect("valid Gitea test configuration"),
    );

    assert!(matches!(
        catalog.install_bundle("broken-hook", "v1.0.0").await,
        Err(CatalogError::ContentInvalid)
    ));
}

async fn spawn_catalog_server(
    repository: &'static str,
    commit_sha: String,
    tree_paths: Vec<&'static str>,
    contents: Vec<(&'static str, &'static str)>,
) -> String {
    spawn_catalog_server_bytes(
        repository,
        commit_sha,
        tree_paths,
        contents
            .into_iter()
            .map(|(path, content)| (path, content.as_bytes().to_vec()))
            .collect(),
    )
    .await
}

async fn spawn_catalog_server_bytes(
    repository: &'static str,
    commit_sha: String,
    tree_paths: Vec<&'static str>,
    contents: Vec<(&'static str, Vec<u8>)>,
) -> String {
    use axum::{
        extract::Path, http::StatusCode, response::IntoResponse, routing::get, Json, Router,
    };
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    use std::{collections::HashMap, sync::Arc};

    let files = Arc::new(
        contents
            .into_iter()
            .map(|(path, content)| (path.to_string(), content))
            .collect::<HashMap<_, _>>(),
    );
    let tree = Arc::new(
        tree_paths
            .into_iter()
            .map(|path| serde_json::json!({ "path": path, "type": "blob" }))
            .collect::<Vec<_>>(),
    );
    let tag_commit_sha = commit_sha.clone();
    let app = Router::new()
        .route(
            "/api/v1/orgs/agents/repos",
            get(move || async move { Json(serde_json::json!([{ "name": repository }])) }),
        )
        .route(
            "/api/v1/repos/agents/:repo/tags",
            get(move || {
                let commit_sha = tag_commit_sha.clone();
                async move {
                    let commit_sha_for_commit = commit_sha.clone();
                    Json(serde_json::json!([{
                        "name": "v1.0.0",
                        "id": commit_sha,
                        "commit": {
                            "sha": commit_sha_for_commit,
                            "created": "2026-10-21T08:30:00Z"
                        }
                    }]))
                }
            }),
        )
        .route(
            "/api/v1/repos/agents/:repo/git/trees/:commit_sha",
            get({
                let tree = tree.clone();
                move || {
                    let tree = tree.clone();
                    async move { Json(serde_json::json!({ "tree": tree.as_ref() })) }
                }
            }),
        )
        .route(
            "/api/v1/repos/agents/:repo/contents/*path",
            get({
                let files = files.clone();
                move |Path((_repo, path)): Path<(String, String)>| {
                    let files = files.clone();
                    async move {
                        let path = path.trim_start_matches('/');
                        match files.get(path) {
                            Some(content) => Json(serde_json::json!({
                                "content": BASE64.encode(content),
                                "encoding": "base64"
                            }))
                            .into_response(),
                            None => (
                                StatusCode::NOT_FOUND,
                                Json(serde_json::json!({ "message": "not found" })),
                            )
                                .into_response(),
                        }
                    }
                }
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
    format!("http://{address}")
}

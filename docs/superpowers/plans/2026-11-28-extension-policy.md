# 扩展必装与弃用策略实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用 Gitea topic 标记扩展包的 `required` / `deprecated` 策略，让客户端自动补齐必装包、自动卸载弃用包，并把安装冲突判定统一成“记录即归属”的一套实现。

**Architecture:** 协议在 `ExtensionSummary` 上增加 `policy`，服务端扫描时读 topic 并把策略随目录缓存；客户端的四态归属判定收进 `extensions/ownership.rs`，安装、更新、卸载、策略同步共用；自动动作收进 `extensions/policy.rs`，由现有后台检查和手动刷新触发。

**Tech Stack:** Rust（`prelay-protocol`、`prelay-server` Axum/reqwest、`prelay-client` Tauri 2）、Nuxt 4 + Bun、`@stellar/ui`。

**Spec:** `prelay-server/docs/superpowers/specs/2026-11-27-extension-policy-design.md`

## Global Constraints

- 归属判定只有一份实现：`prelay-client/src-tauri/src/extensions/ownership.rs`。禁止在 `skills.rs`、`mcp.rs`、`hooks.rs`、`install.rs` 各自保留一套。
- 错误码分工固定：`local_extensions_conflict` = 记录归其他包，不可覆盖；`extension_target_exists` = 有内容但无记录，可经确认覆盖。
- 自动安装一律 `overwrite=false`，不弹确认、不覆盖、不重试覆盖。
- 策略不落本地状态；每次以成功读取的服务端目录为准。新增状态文件需要重新评审。
- 弃用包仍出现在 `/api/extensions/plugins`，但 `versions`、`README`、`install` 返回 `extension_not_found`。
- 不改服务端路由结构，不加数据库表，不改版本标签与安装包结构。
- 不推送、不发布、不打 tag；三个仓库分别提交。
- 每个任务结束执行该仓的格式、lint、测试命令，并 `git diff --check`。

---

### Task 1: 协议增加策略字段

**Files:**
- Modify: `prelay-protocol/src/extensions/mod.rs:32-90`（`ExtensionKind` 之后、`CatalogKind` 之前插入枚举；`ExtensionSummary` 增加字段）
- Modify: `prelay-protocol/src/lib.rs:15`（re-export 列表加 `ExtensionPolicy`）
- Test: `prelay-protocol/tests/extensions_dto.rs`

**Interfaces:**
- Produces: `prelay_protocol::ExtensionPolicy { Normal, Required, Deprecated }`，`Default = Normal`，未知字符串反序列化为 `Normal`。
- Produces: `ExtensionSummary.policy: ExtensionPolicy`，序列化字段名 `policy`，值 `"normal" | "required" | "deprecated"`。

- [ ] **Step 1: 写失败测试**

在 `prelay-protocol/tests/extensions_dto.rs` 末尾追加：

```rust
#[test]
fn extension_policy_defaults_to_normal_and_ignores_unknown_values() {
    use prelay_protocol::{ExtensionPolicy, ExtensionSummary};

    let base = serde_json::json!({
        "name": "demo",
        "repository": "https://git.example.test/agents/demo",
        "latest": {
            "tag": "v1.0.0",
            "commitSha": "a".repeat(40),
            "updatedAt": "2026-11-27T00:00:00Z"
        }
    });
    let missing: ExtensionSummary = serde_json::from_value(base.clone()).unwrap();
    assert_eq!(missing.policy, ExtensionPolicy::Normal);

    let mut unknown = base.clone();
    unknown["policy"] = serde_json::json!("archived");
    let unknown: ExtensionSummary = serde_json::from_value(unknown).unwrap();
    assert_eq!(unknown.policy, ExtensionPolicy::Normal);

    let mut required = base.clone();
    required["policy"] = serde_json::json!("required");
    let required: ExtensionSummary = serde_json::from_value(required).unwrap();
    assert_eq!(required.policy, ExtensionPolicy::Required);
    assert_eq!(
        serde_json::to_value(&required).unwrap()["policy"],
        serde_json::json!("required")
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --all-targets --all-features extension_policy_defaults_to_normal`
Expected: 编译失败，`no variant or associated item named 'policy'` / `unresolved import`.

- [ ] **Step 3: 实现枚举与字段**

`prelay-protocol/src/extensions/mod.rs`，在 `ExtensionKind` 的 `Deserialize` 实现之后加入：

```rust
/// 插件包的生命周期策略，由服务端从 Gitea topic 推导。
///
/// 未知值按 [`ExtensionPolicy::Normal`] 解析，保证服务端新增策略时旧客户端仍能读取目录。
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionPolicy {
    #[default]
    Normal,
    Required,
    Deprecated,
}

impl<'de> Deserialize<'de> for ExtensionPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "required" => Self::Required,
            "deprecated" => Self::Deprecated,
            _ => Self::Normal,
        })
    }
}
```

`ExtensionSummary` 增加字段，放在 `artifacts` 之后：

```rust
    #[serde(default)]
    pub policy: ExtensionPolicy,
```

`prelay-protocol/src/lib.rs` 的 `pub use extensions::{...}` 列表加入 `ExtensionPolicy`。

若 `tests/extensions_dto.rs` 已有 `ExtensionSummary { .. }` 字面量构造，补 `policy: ExtensionPolicy::Normal`，不要给字段加 `#[non_exhaustive]` 规避。

- [ ] **Step 4: 运行协议检查**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features && git diff --check`
Expected: 全部通过。

- [ ] **Step 5: 提交协议仓**

```text
git -C prelay-protocol add src/extensions/mod.rs src/lib.rs tests/extensions_dto.rs
git -C prelay-protocol commit -m "协议增加扩展策略字段"
```

---

### Task 2: 服务端读取 topic 并标注策略

**Files:**
- Modify: `prelay-server/crates/protocol`（submodule 指针）
- Modify: `prelay-server/src/outbound/integrations/extensions/gitea.rs`
- Modify: `prelay-server/src/outbound/integrations/extensions/package.rs`
- Modify: `prelay-server/src/outbound/integrations/extensions/mod.rs`（re-export）
- Modify: `prelay-server/src/outbound/integrations/extensions/catalog.rs`
- Test: `prelay-server/tests/extensions/catalog.rs`

**Interfaces:**
- Consumes: `prelay_protocol::ExtensionPolicy`（Task 1）。
- Produces: `GiteaClient::topics(repository) -> Result<Vec<String>>`。
- Produces: `policy_from_topics(&[String]) -> TopicPolicy`，`TopicPolicy { Normal, Required, Deprecated, Conflict }`。

- [ ] **Step 1: 同步 submodule 指针**

```text
git -C prelay-server submodule update --remote crates/protocol
git -C prelay-server/crates/protocol rev-parse HEAD
```

Expected: HEAD 等于 Task 1 的协议提交。

- [ ] **Step 2: 写失败测试**

在 `prelay-server/tests/extensions/catalog.rs` 追加（复用文件内已有的 `axum` mock 写法）：

```rust
#[test]
fn topic_policy_maps_reserved_topics_and_detects_conflicts() {
    use prelay_server::outbound::integrations::extensions::{policy_from_topics, TopicPolicy};

    assert_eq!(policy_from_topics(&[]), TopicPolicy::Normal);
    assert_eq!(policy_from_topics(&["docs".into()]), TopicPolicy::Normal);
    assert_eq!(policy_from_topics(&["required".into()]), TopicPolicy::Required);
    assert_eq!(
        policy_from_topics(&["deprecated".into()]),
        TopicPolicy::Deprecated
    );
    assert_eq!(
        policy_from_topics(&["required".into(), "deprecated".into()]),
        TopicPolicy::Conflict
    );
    assert_eq!(
        policy_from_topics(&["Required".into()]),
        TopicPolicy::Normal,
        "topic 按小写精确匹配"
    );
}

#[tokio::test]
async fn lists_deprecated_packages_but_rejects_their_install_bundle() {
    use axum::{routing::get, Json, Router};

    let app = Router::new()
        .route(
            "/api/v1/orgs/agents/repos",
            get(|| async { Json(serde_json::json!([{ "name": "old-plugin" }])) }),
        )
        .route(
            "/api/v1/repos/agents/old-plugin/topics",
            get(|| async { Json(serde_json::json!({ "topics": ["deprecated"] })) }),
        )
        .route(
            "/api/v1/repos/agents/old-plugin/tags",
            get(|| async {
                Json(serde_json::json!([{
                    "name": "v1.0.0",
                    "id": "a".repeat(40),
                    "commit": { "sha": "a".repeat(40), "created": "2026-11-27T00:00:00Z" }
                }]))
            }),
        )
        .route(
            "/api/v1/repos/agents/old-plugin/git/trees/:commit_sha",
            get(|| async {
                Json(serde_json::json!({
                    "tree": [{ "path": "skills/demo/SKILL.md", "type": "blob" }]
                }))
            }),
        );
    let catalog = serve_catalog(app).await;

    let plugins = catalog.list(CatalogKind::Plugin).await.unwrap();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].policy, prelay_protocol::ExtensionPolicy::Deprecated);

    let error = catalog
        .install_bundle("old-plugin", "v1.0.0")
        .await
        .unwrap_err();
    assert_eq!(error, CatalogError::ExtensionNotFound);
}

async fn serve_catalog(app: axum::Router) -> ExtensionCatalog {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Gitea test server");
    let address = listener.local_addr().expect("read test server address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve Gitea test server");
    });
    ExtensionCatalog::new(
        reqwest::Client::new(),
        ExtensionCatalogConfig::new(format!("http://{address}"), "agents", None, 300)
            .expect("valid Gitea test configuration"),
    )
}
```

同时给文件内已有的每个 mock 路由补一条空的 topics 路由，例如在 `"/api/v1/orgs/agents/repos"` 之后插入：

```rust
        .route(
            "/api/v1/repos/agents/engineering-rules/topics",
            get(|| async { Json(serde_json::json!({ "topics": [] })) }),
        )
```

每个仓库名各补一条，否则 topic 拉取失败会让整次刷新失败。

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test --test extensions catalog`
Expected: 编译失败，`policy_from_topics` / `TopicPolicy` / `ExtensionSummary.policy` 不存在。

- [ ] **Step 4: 实现 topic 读取与策略解析**

`gitea.rs` 增加方法（放在 `versions` 之后）：

```rust
    pub(super) async fn topics(&self, repository: &str) -> Result<Vec<String>> {
        let topics: GiteaTopics = self
            .get_json(
                &format!("repos/{}/{repository}/topics", self.config.organization()),
                &[],
            )
            .await?;
        Ok(topics.topics)
    }
```

文件末尾新增 DTO：

```rust
#[derive(Deserialize)]
struct GiteaTopics {
    topics: Vec<String>,
}
```

`package.rs` 增加：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicPolicy {
    Normal,
    Required,
    Deprecated,
    Conflict,
}

pub fn policy_from_topics(topics: &[String]) -> TopicPolicy {
    let required = topics.iter().any(|topic| topic == "required");
    let deprecated = topics.iter().any(|topic| topic == "deprecated");
    match (required, deprecated) {
        (true, true) => TopicPolicy::Conflict,
        (true, false) => TopicPolicy::Required,
        (false, true) => TopicPolicy::Deprecated,
        (false, false) => TopicPolicy::Normal,
    }
}
```

`extensions/mod.rs` re-export 加 `policy_from_topics, TopicPolicy`。

- [ ] **Step 5: 扫描时套用策略**

`catalog.rs` 的 `scan()` 改为：每个仓库先取 topics，再取最新版本、文件树与分类，最后合成 `ExtensionPolicy`。核心改动：

```rust
        for repository in self.gitea.repositories().await? {
            if !valid_repository_name(&repository) {
                continue;
            }
            let topics = self
                .gitea
                .topics(&repository)
                .await
                .map_err(|_| CatalogError::Unavailable)?;
            let topic_policy = policy_from_topics(&topics);
            let Some(latest) = versions_from_tags(self.gitea.versions(&repository).await?)
                .into_iter()
                .next()
            else {
                if topic_policy == TopicPolicy::Required {
                    return Err(CatalogError::Unavailable);
                }
                if topic_policy == TopicPolicy::Deprecated {
                    tracing::warn!(repository = %repository, "deprecated package has no release tag");
                }
                continue;
            };
            let paths = self
                .gitea
                .tree_paths(&repository, &latest.commit_sha)
                .await?;
            let classification = match self.classify_version(&paths).await {
                Ok(Some(classification)) => classification,
                Ok(None) => continue,
                Err(error) => {
                    if topic_policy == TopicPolicy::Required {
                        return Err(CatalogError::Unavailable);
                    }
                    tracing::warn!(
                        repository = %repository,
                        ?error,
                        "skipping extension package with invalid content"
                    );
                    continue;
                }
            };
            let policy = match (classification.catalog_kind, topic_policy) {
                (CatalogKind::Rule, TopicPolicy::Normal) => ExtensionPolicy::Normal,
                (CatalogKind::Rule, _) => {
                    tracing::warn!(repository = %repository, "rule package ignores topic policy");
                    ExtensionPolicy::Normal
                }
                (CatalogKind::Plugin, TopicPolicy::Conflict) => {
                    tracing::warn!(repository = %repository, "package has conflicting topic policy");
                    continue;
                }
                (CatalogKind::Plugin, TopicPolicy::Normal) => ExtensionPolicy::Normal,
                (CatalogKind::Plugin, TopicPolicy::Required) => ExtensionPolicy::Required,
                (CatalogKind::Plugin, TopicPolicy::Deprecated) => ExtensionPolicy::Deprecated,
            };
            entries.push(CatalogEntry {
                summary: ExtensionSummary {
                    name: repository.clone(),
                    repository: format!(
                        "{}/{}/{}",
                        self.config.gitea_url(),
                        self.config.organization(),
                        repository
                    ),
                    latest,
                    catalog_kind: Some(classification.catalog_kind),
                    artifacts: classification.artifact_summaries(),
                    policy,
                },
            });
        }
```

`entry()` 只接受可安装包：

```rust
            .find(|entry| {
                entry.summary.name == repository
                    && entry.summary.policy != ExtensionPolicy::Deprecated
            })
```

`refresh()` 在写入缓存前比较策略变化并记录 info 日志：

```rust
    async fn refresh(&self) -> Result<Vec<CatalogEntry>, CatalogError> {
        let _guard = self.refresh_lock.lock().await;
        let entries = self.scan().await.map_err(|_| CatalogError::Unavailable)?;
        let previous = self
            .cache
            .read()
            .await
            .as_ref()
            .map(|cached| cached.entries.clone())
            .unwrap_or_default();
        for entry in &entries {
            let before = previous
                .iter()
                .find(|item| item.summary.name == entry.summary.name)
                .map(|item| item.summary.policy);
            if before != Some(entry.summary.policy) {
                tracing::info!(
                    repository = %entry.summary.name,
                    policy = ?entry.summary.policy,
                    "extension policy updated"
                );
            }
        }
        *self.cache.write().await = Some(CachedCatalog {
            loaded_at: Instant::now(),
            entries: entries.clone(),
        });
        Ok(entries)
    }
```

- [ ] **Step 6: 运行服务端检查**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --test extensions && git diff --check`
Expected: 全部通过。`TEST_POSTGRES_URL` 未设置的集成测试仍按仓库既有方式失败，记录缺口，不写成功。

- [ ] **Step 7: 提交服务端**

```text
git -C prelay-server add crates/protocol src/outbound/integrations/extensions tests/extensions
git -C prelay-server commit -m "目录按 topic 标注扩展策略"
```

---

### Task 3: 客户端目录投影策略并过滤弃用包

**Files:**
- Modify: `prelay-client/crates/protocol`（submodule 指针）
- Modify: `prelay-client/src-tauri/src/extensions/model.rs:10-24`
- Modify: `prelay-client/src-tauri/src/extensions/catalog.rs:21-62`

**Interfaces:**
- Consumes: `prelay_protocol::ExtensionPolicy`（Task 1）。
- Produces: `ExtensionPackage.policy: ExtensionPolicy`。
- Produces: `pub(crate) fn packages_from_summaries(summaries: Vec<ExtensionSummary>, kind: CatalogKind) -> Vec<ExtensionPackage>`，只返回非 `Deprecated` 包。
- Produces: `pub(crate) async fn load_summaries(client, kind) -> Result<Vec<ExtensionSummary>, ClientError>`（改为 `pub(crate)`）。

- [ ] **Step 1: 同步 submodule 指针**

```text
git -C prelay-client submodule update --remote crates/protocol
git -C prelay-client/crates/protocol rev-parse HEAD
```

- [ ] **Step 2: 写失败测试**

在 `prelay-client/src-tauri/src/extensions/catalog.rs` 的 `mod tests` 内追加：

```rust
    #[test]
    fn display_projection_drops_deprecated_packages_and_keeps_policy() {
        use prelay_protocol::{ExtensionPolicy, ExtensionSummary, ExtensionVersion};

        let summary = |name: &str, policy: ExtensionPolicy| ExtensionSummary {
            name: name.to_string(),
            repository: format!("https://example.test/agents/{name}"),
            latest: ExtensionVersion {
                tag: "v1.0.0".to_string(),
                commit_sha: "a".repeat(40),
                updated_at: "2026-11-27T00:00:00Z".to_string(),
            },
            catalog_kind: Some(CatalogKind::Plugin),
            artifacts: Vec::new(),
            policy,
        };

        let packages = packages_from_summaries(
            vec![
                summary("keep", ExtensionPolicy::Normal),
                summary("must", ExtensionPolicy::Required),
                summary("gone", ExtensionPolicy::Deprecated),
            ],
            CatalogKind::Plugin,
        );

        assert_eq!(
            packages
                .iter()
                .map(|package| package.name.as_str())
                .collect::<Vec<_>>(),
            vec!["keep", "must"]
        );
        assert_eq!(packages[1].policy, ExtensionPolicy::Required);
    }
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test --lib display_projection_drops_deprecated`
Expected: 编译失败，`packages_from_summaries` 不存在、`policy` 字段不存在。

- [ ] **Step 4: 实现模型与投影**

`model.rs` 的 `ExtensionPackage` 增加：

```rust
    #[serde(default)]
    pub policy: prelay_protocol::ExtensionPolicy,
```

`catalog.rs` 抽出投影函数并把 `load_summaries` 提升为 `pub(crate)`：

```rust
use prelay_protocol::ExtensionPolicy;

pub(crate) fn packages_from_summaries(
    summaries: Vec<ExtensionSummary>,
    kind: CatalogKind,
) -> Vec<ExtensionPackage> {
    summaries
        .into_iter()
        .filter(|summary| summary.policy != ExtensionPolicy::Deprecated)
        .map(|summary| ExtensionPackage {
            name: summary.name,
            repository: summary.repository,
            commit_sha: summary.latest.commit_sha,
            version: summary.latest.tag,
            catalog_kind: summary.catalog_kind.unwrap_or(kind),
            artifacts: summary.artifacts,
            install_action: ExtensionInstallAction::Install,
            installed_clients: Vec::new(),
            policy: summary.policy,
        })
        .collect()
}
```

`list_extensions` 改为调用它，并保持后续安装状态计算不变。

- [ ] **Step 5: 运行客户端原生层检查**

Run（`prelay-client/src-tauri`）: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features`
Expected: 通过；`ExtensionPackage` 的其他构造点（测试辅助、install 反序列化）同步补 `policy`。

- [ ] **Step 6: 提交客户端**

```text
git -C prelay-client add crates/protocol src-tauri/src/extensions
git -C prelay-client commit -m "目录投影扩展策略并过滤弃用包"
```

---

### Task 4: 抽出 ownership 模块，统一四个预检

**Files:**
- Create: `prelay-client/src-tauri/src/extensions/ownership.rs`
- Modify: `prelay-client/src-tauri/src/extensions/mod.rs:1-16`
- Modify: `prelay-client/src-tauri/src/extensions/skills.rs:140-190`
- Modify: `prelay-client/src-tauri/src/extensions/mcp.rs:29-72`
- Modify: `prelay-client/src-tauri/src/extensions/hooks.rs:126-172`
- Modify: `prelay-client/src-tauri/src/extensions/install.rs:141-160`
- Test: `prelay-client/src-tauri/src/extensions/install_state_tests.rs`、`install_state_tests/rule_targets.rs`

**Interfaces:**
- Produces: `ownership::Owner<'a> { Owned, Other(&'a str), None }`。
- Produces: `ownership::skill_owner(state, package, target_root, name)`、`mcp_owner(state, package, server_name)`、`hook_owner(state, package, handler_hash)`、`rule_owner(state, package, target)`。
- Produces: `ownership::owned_elsewhere(package) -> ClientError`（`local_extensions_conflict`）、`ownership::unmanaged_exists() -> ClientError`（`extension_target_exists`）。

- [ ] **Step 1: 写失败测试**

`install_state_tests.rs` 追加两个测试与一个辅助函数（放在 `file` 辅助函数之后）：

```rust
fn skill_artifact(name: &str) -> ExtensionArtifact {
    ExtensionArtifact {
        kind: ExtensionKind::Skill,
        files: vec![file(&format!("skills/{name}/SKILL.md"), "# Managed\n")],
    }
}

#[test]
fn refuses_to_take_over_another_packages_skill_even_with_overwrite() {
    let directory = tempdir().unwrap();
    let home = directory.path();
    let first = bundle("first", vec![skill_artifact("foo")]);
    install_prepared_bundle(home, &first, &[AgentClient::CodexCli], false).unwrap();

    let second = bundle("second", vec![skill_artifact("foo")]);
    let error =
        install_prepared_bundle(home, &second, &[AgentClient::CodexCli], true).unwrap_err();
    assert_eq!(error.code(), "local_extensions_conflict");
    assert_eq!(
        fs::read_to_string(home.join(".agents/skills/foo/SKILL.md")).unwrap(),
        "# Managed\n"
    );
}
```

`install_state_tests/rule_targets.rs` 把现有 `replaces_another_rule_package_only_with_overwrite` 改成：

```rust
#[test]
fn refuses_to_take_over_another_rule_package_even_with_overwrite() {
    let directory = tempdir().unwrap();
    let home = directory.path();
    let first = bundle("first", vec![rule_artifact("# First\n")]);
    install_prepared_bundle(home, &first, &[AgentClient::CodexCli], false).unwrap();

    let second = bundle("second", vec![rule_artifact("# Second\n")]);
    let error =
        install_prepared_bundle(home, &second, &[AgentClient::CodexCli], true).unwrap_err();
    assert_eq!(error.code(), "local_extensions_conflict");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib refuses_to_take_over`
Expected: 两个测试失败；Skill 与规则目前都返回 `extension_target_exists`。

- [ ] **Step 3: 新建 ownership 模块**

`prelay-client/src-tauri/src/extensions/ownership.rs`：

```rust
use std::path::Path;

use crate::relay::client::ClientError;

use super::host_state::{normalize_path, HostState, PluginArtifact};

/// 目标制品的归属判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner<'a> {
    /// 本包记录已声明。
    Owned,
    /// 另一个已安装插件声明。
    Other(&'a str),
    /// 没有任何记录声明。
    None,
}

pub(crate) fn skill_owner<'a>(
    state: &'a HostState,
    package: &str,
    target_root: &Path,
    name: &str,
) -> Owner<'a> {
    let target = normalize_path(target_root);
    owner_of(state, package, |artifact| match artifact {
        PluginArtifact::Skill {
            target: artifact_target,
            names,
        } => artifact_target == &target && names.iter().any(|candidate| candidate == name),
        _ => false,
    })
}

pub(crate) fn mcp_owner<'a>(
    state: &'a HostState,
    package: &str,
    server_name: &str,
) -> Owner<'a> {
    owner_of(state, package, |artifact| {
        matches!(
            artifact,
            PluginArtifact::Mcp { server_name: artifact_name, .. } if artifact_name == server_name
        )
    })
}

pub(crate) fn hook_owner<'a>(
    state: &'a HostState,
    package: &str,
    handler_hash: &str,
) -> Owner<'a> {
    owner_of(state, package, |artifact| {
        matches!(
            artifact,
            PluginArtifact::Hook { handler_hash: artifact_hash, .. } if artifact_hash == handler_hash
        )
    })
}

pub(crate) fn rule_owner<'a>(
    state: &'a HostState,
    package: &str,
    target: &Path,
) -> Owner<'a> {
    match state.rule_entry(target) {
        Some(entry) if entry.package == package => Owner::Owned,
        Some(entry) => Owner::Other(entry.package.as_str()),
        None => Owner::None,
    }
}

fn owner_of<'a>(
    state: &'a HostState,
    package: &str,
    matches: impl Fn(&PluginArtifact) -> bool,
) -> Owner<'a> {
    let owned = state
        .plugin(package)
        .is_some_and(|plugin| plugin.artifacts.iter().any(&matches));
    if owned {
        return Owner::Owned;
    }
    for plugin in state.active_plugins() {
        if plugin.name == package {
            continue;
        }
        if plugin.artifacts.iter().any(&matches) {
            return Owner::Other(plugin.name.as_str());
        }
    }
    Owner::None
}

pub(crate) fn owned_elsewhere(package: &str) -> ClientError {
    ClientError::new(
        "local_extensions_conflict",
        format!("目标由扩展包 {package} 管理，请先卸载该扩展包。"),
    )
}

pub(crate) fn unmanaged_exists() -> ClientError {
    ClientError::new(
        "extension_target_exists",
        "目标位置已有未受管内容，确认后可覆盖安装。",
    )
}
```

`extensions/mod.rs` 加入 `pub(crate) mod ownership;`。

- [ ] **Step 4: 四个预检改调 ownership**

`skills.rs` 的 `ensure_available` 整体替换为：

```rust
fn ensure_available(
    target_root: &Path,
    package: &str,
    roots: &BTreeSet<String>,
    overwrite: bool,
    state: &HostState,
) -> Result<(), ClientError> {
    for root in roots {
        match ownership::skill_owner(state, package, target_root, root) {
            Owner::Owned => {}
            Owner::Other(name) => return Err(ownership::owned_elsewhere(name)),
            Owner::None => {
                if !overwrite && target_root.join(root).exists() {
                    return Err(ownership::unmanaged_exists());
                }
            }
        }
    }
    Ok(())
}
```

文件头 `use super::{...}` 增加 `ownership::{self, Owner}`，并删除不再使用的 `PluginArtifact`/`HostState` 导入项中多余部分（保留 `HostState`，`PluginArtifact` 只在其他函数用；编译告警为准）。

`mcp.rs` 的 `preflight_mcp_install` 中删除 `owned` 与 `owned_by_other` 两段，替换为：

```rust
    match ownership::mcp_owner(state, package, &expanded.name) {
        Owner::Owned => {}
        Owner::Other(name) => return Err(ownership::owned_elsewhere(name)),
        Owner::None => {
            if exists && !overwrite {
                return Err(ownership::unmanaged_exists());
            }
        }
    }
```

`hooks.rs` 的 `preflight_hook_install` 删除 `previous` 与 `owned_by_other` 两段，循环体改为：

```rust
    for handler in incoming {
        match ownership::hook_owner(state, package, &handler.hash) {
            Owner::Owned => {}
            Owner::Other(name) => return Err(ownership::owned_elsewhere(name)),
            Owner::None => {
                if !overwrite && existing.contains(&handler.hash) {
                    return Err(ownership::unmanaged_exists());
                }
            }
        }
    }
```

`install.rs` 的规则分支替换为：

```rust
        match ownership::rule_owner(state, &bundle.name, &target) {
            Owner::Owned => {}
            Owner::Other(name) => return Err(ownership::owned_elsewhere(name)),
            Owner::None => {
                if target.exists() && !overwrite {
                    return Err(ownership::unmanaged_exists());
                }
            }
        }
```

注意 `install_rule_bundle` 目前没有 `state` 参数；改为在函数开头 `let state = HostState::load_locked(home, host)?;`，`rules_state` 仍从同一个 state 取 `rules` 克隆。调用点 `install_prepared_bundle` 已经加载过 state，改为把 `&state` 传进去，避免二次读盘。

签名调整为 `fn install_rule_bundle(home, host, clients, bundle, overwrite, state: &HostState)`，函数内 `let mut rules_state = state.rules.clone();`，调用点改为 `install_rule_bundle(home, host, clients, bundle, overwrite, &state)?;`。

- [ ] **Step 5: 运行测试**

Run（`prelay-client/src-tauri`）: `cargo fmt --all && cargo test --all-targets --all-features`
Expected: 新测试通过，既有安装/卸载测试除错误码断言外保持通过。

- [ ] **Step 6: 提交**

```text
git -C prelay-client add src-tauri/src/extensions
git -C prelay-client commit -m "统一扩展安装冲突判定"
```

---

### Task 5: 取消受管制品的内容修改检查

**Files:**
- Modify: `prelay-client/src-tauri/src/extensions/install/cleanup.rs:11-35`（删除 `preflight_obsolete`）
- Modify: `prelay-client/src-tauri/src/extensions/install.rs:27`、`:281-284`（删除导入与调用）
- Modify: `prelay-client/src-tauri/src/extensions/uninstall.rs:70-90`（删除 `preflight` 与调用）
- Test: `prelay-client/src-tauri/src/extensions/install_state_tests.rs:301-345`

**Interfaces:**
- 变更后：`uninstall_plugin_for_client` 不再因 MCP 被改过而返回 `local_extensions_conflict`。
- 变更后：更新时旧制品直接删除，不做修改检查。

- [ ] **Step 1: 改写测试**

把 `refuses_to_uninstall_an_mcp_modified_by_the_user` 整体替换为：

```rust
#[test]
fn uninstalls_an_mcp_modified_by_the_user() {
    let directory = tempdir().unwrap();
    let home = directory.path();
    let manifest = json!({
        "name": "filesystem",
        "transport": {
            "type": "stdio",
            "command": ["uvx", "mcp-server-filesystem"],
            "cwd": null,
            "environment": {},
            "enabled": true,
            "timeoutMs": null
        }
    })
    .to_string();
    let demo = bundle(
        "demo",
        vec![ExtensionArtifact {
            kind: ExtensionKind::Mcp,
            files: vec![file("server.json", &manifest)],
        }],
    );
    install_prepared_bundle(home, &demo, &[AgentClient::CodexCli], false).unwrap();
    let config_path = home.join(".codex").join("config.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(&config_path, config.replace("uvx", "other")).unwrap();

    uninstall_plugin_for_client(home, AgentClient::CodexCli, "demo").unwrap();
    assert!(HostState::load(home, ExtensionHost::Codex)
        .unwrap()
        .plugin("demo")
        .is_none());
    assert!(!fs::read_to_string(&config_path)
        .unwrap()
        .contains("[mcp_servers.filesystem]"));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --lib uninstalls_an_mcp_modified_by_the_user`
Expected: FAIL，返回 `local_extensions_conflict`。

- [ ] **Step 3: 删除检查**

- 删除 `install/cleanup.rs` 的 `preflight_obsolete` 与 `ArtifactHealth`/`mcp` 导入。
- `install.rs` 删除 `use cleanup::{preflight_obsolete, remove_obsolete};` 中的 `preflight_obsolete`，并删除 `if let Some(previous) = &previous { preflight_obsolete(home, previous, &planned)?; }` 代码块。
- 删除 `uninstall.rs` 的 `fn preflight(...)` 与调用 `preflight(home, &record)?;`，清理只被它使用的 `ArtifactHealth` 导入；`mcp` 仍用于卸载循环，保留。

- [ ] **Step 4: 运行测试**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features`
Expected: 全部通过。

- [ ] **Step 5: 提交**

```text
git -C prelay-client add src-tauri/src/extensions
git -C prelay-client commit -m "更新与卸载不再校验受管内容"
```

---

### Task 6: 策略同步模块与命令

**Files:**
- Create: `prelay-client/src-tauri/src/extensions/policy.rs`
- Modify: `prelay-client/src-tauri/src/extensions/mod.rs`
- Modify: `prelay-client/src-tauri/src/extensions/uninstall.rs`（抽出按宿主卸载）
- Modify: `prelay-client/src-tauri/src/extensions/checks.rs:24-58`
- Modify: `prelay-client/src-tauri/src/commands/extensions.rs`
- Modify: `prelay-client/src-tauri/src/app/mod.rs:50-53`
- Test: `prelay-client/src-tauri/src/extensions/policy_tests.rs`

**Interfaces:**
- Produces: `pub struct PolicyOutcome { uninstalled, installed, skipped, failed }`，`Serialize` camelCase；`PolicyFailure { name, code, message }`。
- Produces: `pub(crate) fn plan(home, host, packages, clients) -> Result<PolicyPlan, ClientError>`。
- Produces: `pub(crate) fn execute(home, host, plan, clients, bundles) -> PolicyOutcome`。
- Produces: `pub(crate) async fn sync(home, state) -> Result<PolicyOutcome, ClientError>`。
- Produces: Tauri command `extensions_policy_sync`。

- [ ] **Step 1: 写失败测试**

`extensions/policy_tests.rs`：

```rust
use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use prelay_protocol::{
    CatalogKind, ExtensionArtifact, ExtensionFile, ExtensionInstallBundle, ExtensionKind,
    ExtensionPolicy, ExtensionVersion,
};
use tempfile::tempdir;

use crate::agents::AgentClient;

use super::{
    host_state::{ExtensionHost, HostState},
    install::install_prepared_bundle,
    model::ExtensionPackage,
    policy::{execute, plan},
};

fn skill_bundle(name: &str, tag: &str, skill: &str) -> ExtensionInstallBundle {
    ExtensionInstallBundle {
        name: name.to_string(),
        version: ExtensionVersion {
            tag: tag.to_string(),
            commit_sha: "abc".to_string(),
            updated_at: "2026-11-27T00:00:00Z".to_string(),
        },
        artifacts: vec![ExtensionArtifact {
            kind: ExtensionKind::Skill,
            files: vec![ExtensionFile {
                path: format!("skills/{skill}/SKILL.md"),
                content_base64: BASE64.encode("# Managed\n"),
            }],
        }],
    }
}

fn package(name: &str, tag: &str, policy: ExtensionPolicy) -> ExtensionPackage {
    ExtensionPackage {
        name: name.to_string(),
        repository: format!("https://example.test/agents/{name}"),
        commit_sha: "abc".to_string(),
        version: tag.to_string(),
        catalog_kind: CatalogKind::Plugin,
        artifacts: Vec::new(),
        install_action: Default::default(),
        installed_clients: Vec::new(),
        policy,
    }
}

#[test]
fn uninstalls_a_deprecated_package_before_installing() {
    let directory = tempdir().unwrap();
    let home = directory.path();
    let clients = [AgentClient::CodexCli];
    install_prepared_bundle(home, &skill_bundle("old", "v1.0.0", "shared"), &clients, false)
        .unwrap();
    let packages = vec![package("old", "v1.0.0", ExtensionPolicy::Deprecated)];

    let planned = plan(home, ExtensionHost::Codex, &packages, &clients).unwrap();
    assert_eq!(planned.uninstall, vec!["old".to_string()]);

    let outcome = execute(
        home,
        ExtensionHost::Codex,
        &planned,
        &clients,
        &BTreeMap::new(),
    );
    assert_eq!(outcome.uninstalled, vec!["old".to_string()]);
    assert!(HostState::load(home, ExtensionHost::Codex)
        .unwrap()
        .plugin("old")
        .is_none());
}

#[test]
fn installs_missing_and_skips_up_to_date_required_packages() {
    let directory = tempdir().unwrap();
    let home = directory.path();
    let clients = [AgentClient::CodexCli];
    let packages = vec![package("must", "v1.0.0", ExtensionPolicy::Required)];
    let bundles = BTreeMap::from([(
        "must".to_string(),
        skill_bundle("must", "v1.0.0", "must"),
    )]);

    let planned = plan(home, ExtensionHost::Codex, &packages, &clients).unwrap();
    assert_eq!(planned.install.len(), 1);
    let outcome = execute(
        home,
        ExtensionHost::Codex,
        &planned,
        &clients,
        &bundles,
    );
    assert_eq!(outcome.installed, vec!["must".to_string()]);

    let planned = plan(home, ExtensionHost::Codex, &packages, &clients).unwrap();
    assert!(planned.install.is_empty());
    assert_eq!(planned.skip[0].name, "must");
    assert_eq!(planned.skip[0].reason, "up-to-date");
}

#[test]
fn repairs_a_required_package_whose_artifact_was_removed() {
    let directory = tempdir().unwrap();
    let home = directory.path();
    let clients = [AgentClient::CodexCli];
    let packages = vec![package("must", "v1.0.0", ExtensionPolicy::Required)];
    let bundles = BTreeMap::from([(
        "must".to_string(),
        skill_bundle("must", "v1.0.0", "must"),
    )]);
    let planned = plan(home, ExtensionHost::Codex, &packages, &clients).unwrap();
    execute(
        home,
        ExtensionHost::Codex,
        &planned,
        &clients,
        &bundles,
    );
    std::fs::remove_dir_all(home.join(".agents").join("skills").join("must")).unwrap();

    let planned = plan(home, ExtensionHost::Codex, &packages, &clients).unwrap();
    assert_eq!(planned.install.len(), 1, "缺失制品应触发补装");
}
```

`extensions/mod.rs` 加 `#[cfg(test)] mod policy_tests;`。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --lib policy_tests`
Expected: 编译失败，`policy` 模块不存在。

- [ ] **Step 3: 抽出按宿主卸载**

`uninstall.rs` 把现有 `uninstall_plugin_for_client` 拆成两层，行为不变：

```rust
pub(crate) fn uninstall_plugin_for_client(
    home: &Path,
    client: AgentClient,
    package: &str,
) -> Result<(), ClientError> {
    uninstall_plugin(home, ExtensionHost::from_client(client), package)
}

pub(crate) fn uninstall_plugin(
    home: &Path,
    host: ExtensionHost,
    package: &str,
) -> Result<(), ClientError> {
    // 原 uninstall_plugin_for_client 的函数体，第一行改为使用传入的 host
}
```

- [ ] **Step 4: 实现 policy 模块**

`policy.rs`：

```rust
use std::{collections::BTreeMap, path::Path};

use prelay_protocol::{CatalogKind, ExtensionInstallBundle, ExtensionPolicy, ExtensionSummary};

use super::{
    catalog::load_summaries,
    hooks,
    host_state::{ArtifactHealth, ExtensionHost, HostState, PluginArtifact, PluginState},
    install::{install_prepared_bundle, validate_package_bundle},
    mcp, skills,
    model::ExtensionPackage,
    uninstall::uninstall_plugin,
};
use crate::{
    agents::{installed_agent_clients, AgentClient},
    identity::registration::authenticated_api,
    relay::client::ClientError,
    NativeState,
};

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyOutcome {
    pub uninstalled: Vec<String>,
    pub installed: Vec<String>,
    pub skipped: Vec<PolicySkip>,
    pub failed: Vec<PolicyFailure>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySkip {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyFailure {
    pub name: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub(crate) struct PolicyPlan {
    pub uninstall: Vec<String>,
    pub install: Vec<ExtensionPackage>,
    pub skip: Vec<PolicySkip>,
}

static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) fn plan(
    home: &Path,
    host: ExtensionHost,
    packages: &[ExtensionPackage],
    clients: &[AgentClient],
) -> Result<PolicyPlan, ClientError> {
    let state = HostState::load(home, host)?;
    let mut plan = PolicyPlan::default();
    for package in packages {
        match package.policy {
            ExtensionPolicy::Normal => {}
            ExtensionPolicy::Deprecated => {
                if state.plugin(&package.name).is_some() {
                    plan.uninstall.push(package.name.clone());
                } else {
                    plan.skip.push(PolicySkip {
                        name: package.name.clone(),
                        reason: "not-installed".to_string(),
                    });
                }
            }
            ExtensionPolicy::Required => {
                if clients.is_empty() {
                    plan.skip.push(PolicySkip {
                        name: package.name.clone(),
                        reason: "no-client".to_string(),
                    });
                    continue;
                }
                if required_needs_install(home, &state, package)? {
                    plan.install.push(package.clone());
                } else {
                    plan.skip.push(PolicySkip {
                        name: package.name.clone(),
                        reason: "up-to-date".to_string(),
                    });
                }
            }
        }
    }
    plan.uninstall.sort();
    plan.install
        .sort_by(|left, right| left.name.cmp(&right.name));
    Ok(plan)
}

fn required_needs_install(
    home: &Path,
    state: &HostState,
    package: &ExtensionPackage,
) -> Result<bool, ClientError> {
    let Some(record) = state.plugin(&package.name) else {
        return Ok(true);
    };
    if record.state != PluginState::Installed
        || record.version != package.version
        || record.commit_sha != package.commit_sha
    {
        return Ok(true);
    }
    for artifact in &record.artifacts {
        let present = match artifact {
            PluginArtifact::Skill { target, names } => {
                skills::health(Path::new(target), names) != ArtifactHealth::Missing
            }
            PluginArtifact::Mcp { server_name, .. } => mcp::server_exists(home, server_name)?,
            PluginArtifact::Hook { handler_hash, .. } => hooks::handlers(home)?
                .iter()
                .any(|handler| &handler.hash == handler_hash),
        };
        if !present {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn execute(
    home: &Path,
    host: ExtensionHost,
    plan: &PolicyPlan,
    clients: &[AgentClient],
    bundles: &BTreeMap<String, ExtensionInstallBundle>,
) -> PolicyOutcome {
    let mut outcome = PolicyOutcome::default();
    for name in &plan.uninstall {
        match uninstall_plugin(home, host, name) {
            Ok(()) => outcome.uninstalled.push(name.clone()),
            Err(error) => outcome.failed.push(failure(name, &error)),
        }
    }
    for package in &plan.install {
        let Some(bundle) = bundles.get(&package.name) else {
            outcome.failed.push(PolicyFailure {
                name: package.name.clone(),
                code: "extension_bundle_missing".to_string(),
                message: "安装包未下载。".to_string(),
            });
            continue;
        };
        if let Err(error) = validate_package_bundle(package, bundle) {
            outcome.failed.push(failure(&package.name, &error));
            continue;
        }
        match install_prepared_bundle(home, bundle, clients, false) {
            Ok(_) => outcome.installed.push(package.name.clone()),
            Err(error) => outcome.failed.push(failure(&package.name, &error)),
        }
    }
    outcome.skipped = plan.skip.clone();
    outcome
}

fn failure(name: &str, error: &ClientError) -> PolicyFailure {
    PolicyFailure {
        name: name.to_string(),
        code: error.code().to_string(),
        message: error.message.clone(),
    }
}

pub(crate) async fn sync(
    home: &Path,
    state: &NativeState,
) -> Result<PolicyOutcome, ClientError> {
    let _guard = SYNC_LOCK.lock().await;
    let clients = installed_agent_clients();
    let host = ExtensionHost::from_clients(&clients).unwrap_or(ExtensionHost::Codex);
    let summaries: Vec<ExtensionSummary> = {
        let client = authenticated_api(state).await?;
        load_summaries(&client, CatalogKind::Plugin).await?
    };
    let packages = summaries
        .iter()
        .map(|summary| ExtensionPackage {
            name: summary.name.clone(),
            repository: summary.repository.clone(),
            commit_sha: summary.latest.commit_sha.clone(),
            version: summary.latest.tag.clone(),
            catalog_kind: summary.catalog_kind.unwrap_or(CatalogKind::Plugin),
            artifacts: summary.artifacts.clone(),
            install_action: Default::default(),
            installed_clients: Vec::new(),
            policy: summary.policy,
        })
        .collect::<Vec<_>>();
    let mut planned = plan(home, host, &packages, &clients)?;
    let mut bundles = BTreeMap::new();
    let mut failures = Vec::new();
    for package in &planned.install {
        let bundle: Result<ExtensionInstallBundle, ClientError> = async {
            let client = authenticated_api(state).await?;
            client
                .get(&format!(
                    "/api/extensions/{}/versions/{}/install",
                    package.name, package.version
                ))
                .await
        }
        .await;
        match bundle {
            Ok(bundle) => {
                bundles.insert(package.name.clone(), bundle);
            }
            Err(error) => failures.push(failure(&package.name, &error)),
        }
    }
    planned
        .install
        .retain(|package| bundles.contains_key(&package.name));
    let mut outcome = execute(home, host, &planned, &clients, &bundles);
    outcome.failed.extend(failures);
    Ok(outcome)
}
```

`mcp.rs` 增加：

```rust
pub(crate) fn server_exists(home: &Path, server_name: &str) -> Result<bool, ClientError> {
    codex_mcp_server_exists(home, server_name)
        .map_err(|error| ClientError::new("local_extensions_error", error))
}
```

- [ ] **Step 5: 接入命令与后台检查**

`commands/extensions.rs` 增加：

```rust
#[tauri::command]
pub async fn extensions_policy_sync(
    state: tauri::State<'_, NativeState>,
) -> Result<crate::extensions::PolicyOutcome, ClientError> {
    let home = user_home()?;
    crate::extensions::sync_policies(&home, &state).await
}
```

`app/mod.rs` 的 `generate_handler!` 列表加入 `crate::commands::extensions::extensions_policy_sync`。

`extensions/mod.rs` re-export：`pub use policy::{sync as sync_policies, PolicyOutcome};`（`PolicyOutcome` 为 `pub`，避免 Tauri 命令的 `private_interfaces` 告警）。

`checks.rs` 在每个检查周期、`for kind` 循环之前跑一次同步，并在有结果时推送 `extensions:policy`。`tokio::time::interval` 的首个 tick 立即触发，因此同一段代码覆盖启动场景，不需要额外的启动调用：

```rust
            // 每个周期只同步一次，规则与插件的目录读取共用这次结果。
            let outcome = crate::extensions::sync_policies(&home, &state).await;
            if let Ok(outcome) = &outcome {
                if !outcome.uninstalled.is_empty()
                    || !outcome.installed.is_empty()
                    || !outcome.failed.is_empty()
                {
                    let _ = app.emit(POLICY_EVENT, outcome);
                }
            }
            for (index, kind) in CHECKED_KINDS.into_iter().enumerate() {
                let Ok(snapshot) = list_extensions(&home, &state, kind).await else {
                    continue;
                };
                let Ok(payload) = serde_json::to_string(&snapshot) else {
                    continue;
                };
                if last_pushed[index].as_deref() == Some(payload.as_str()) {
                    continue;
                }
                last_pushed[index] = Some(payload);
                let _ = app.emit(CATALOG_EVENT, CatalogEvent { kind, snapshot });
            }
```

`const POLICY_EVENT: &str = "extensions:policy";`

- [ ] **Step 6: 运行测试**

Run（`prelay-client/src-tauri`）: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features`
Expected: 通过。

- [ ] **Step 7: 提交**

```text
git -C prelay-client add src-tauri/src
git -C prelay-client commit -m "增加扩展策略同步"
```

---

### Task 7: 前端接策略事件与必装标记

**Files:**
- Modify: `prelay-client/app/types/relay.ts:308-328`
- Modify: `prelay-client/app/composables/extension/updates.ts`
- Modify: `prelay-client/app/composables/extension/updateStream.ts`
- Modify: `prelay-client/app/composables/extension/installJobs.ts:110-150`
- Modify: `prelay-client/app/components/extensions/CatalogTable.vue:186-192`
- Test: `prelay-client/tests/extension-policy-ui.test.ts`（新建）

**Interfaces:**
- Consumes: `ExtensionPackage.policy`（Task 3）、`extensions_policy_sync`（Task 6）、`extensions:policy` 事件。
- Produces: 目录表格对 `required` 显示“必装”标签；刷新按钮先同步策略再读目录。

- [ ] **Step 1: 写失败测试**

`tests/extension-policy-ui.test.ts`，沿用仓库现有的源码断言风格：

```ts
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

test("必装包显示标记并只对未受管内容弹覆盖确认", () => {
  const table = readFileSync(
    new URL("../app/components/extensions/CatalogTable.vue", import.meta.url),
    "utf8",
  );
  const jobs = readFileSync(
    new URL("../app/composables/extension/installJobs.ts", import.meta.url),
    "utf8",
  );
  const updates = readFileSync(
    new URL("../app/composables/extension/updates.ts", import.meta.url),
    "utf8",
  );
  const stream = readFileSync(
    new URL("../app/composables/extension/updateStream.ts", import.meta.url),
    "utf8",
  );

  expect(table).toContain('row.policy === "required"');
  expect(table).toContain("必装");
  expect(jobs).toContain('error.code !== "extension_target_exists"');
  expect(jobs).toContain("local_extensions_conflict");
  expect(updates).toContain("extensions_policy_sync");
  expect(stream).toContain("extensions:policy");
});
```

- [ ] **Step 2: 运行确认失败**

Run: `bun test tests/extension-policy-ui.test.ts`
Expected: FAIL，断言字符串不存在。

- [ ] **Step 3: 类型与界面**

`app/types/relay.ts` 在 `ExtensionPackage` 增加：

```ts
export type ExtensionPolicy = "normal" | "required" | "deprecated";

// ExtensionPackage 内部
  policy: ExtensionPolicy;
```

`CatalogTable.vue` 的名称单元：

```vue
    <template #cell-name="{ row }">
      <span class="extension-catalog-name" :title="row.name">
        {{ row.name }}
        <TagGroup v-if="row.policy === 'required'" :items="['必装']" />
      </span>
    </template>
```

并给 `.extension-catalog-name` 加 `display: inline-flex; align-items: center; gap: var(--spacing-xs);`。

- [ ] **Step 4: 刷新先同步策略**

`updates.ts`：

```ts
  async function refresh(kind: ExtensionCatalogKind) {
    if (kind === "plugin") {
      await invokeLocalCommand("extensions_policy_sync", undefined, {
        notify: false,
        trackPending: false,
      });
    }
    await extensionCatalog.load(kind, true);
  }
```

策略只作用于插件，规则刷新不触发同步，避免一次页面刷新跑两遍。

`updateStream.ts` 增加事件订阅，收到后刷新两个目录：

```ts
export const EXTENSION_POLICY_EVENT = "extensions:policy";

  let unlistenPolicy: UnlistenFn | undefined;

  unlistenPolicy = await listen(EXTENSION_POLICY_EVENT, () => {
    void refresh();
  });
```

`stop()` 内补 `unlistenPolicy?.(); unlistenPolicy = undefined;`。

`installJobs.ts` 的 `catch` 分支增加归属冲突提示：

```ts
        if (error.code === "local_extensions_conflict") {
          notifications.error(errorText(toRelayError(caught)), {
            title: "扩展目标已被其他扩展包占用",
          });
          await refreshCatalog(request.package);
          return;
        }
        if (error.code !== "extension_target_exists") throw caught;
```

- [ ] **Step 5: 运行前端检查**

Run: `bun test && bun run typecheck && bun run lint && bun run format:check`
Expected: 通过。

- [ ] **Step 6: 提交**

```text
git -C prelay-client add app tests
git -C prelay-client commit -m "扩展页展示必装标记并响应策略同步"
```

---

## 完成检查

- [ ] `git -C prelay-protocol status --short`、`git -C prelay-server status --short`、`git -C prelay-client status --short` 只包含本次任务文件。
- [ ] 两个父仓的 `crates/protocol` 指向同一个协议提交。
- [ ] `rg -n "owned_by_other" prelay-client/src-tauri/src` 无结果，确认归属判定没有第二份。
- [ ] `rg -n "preflight_obsolete|ArtifactHealth::Modified" prelay-client/src-tauri/src/extensions` 只剩健康检查展示用途。
- [ ] 未执行推送、发布或 tag。

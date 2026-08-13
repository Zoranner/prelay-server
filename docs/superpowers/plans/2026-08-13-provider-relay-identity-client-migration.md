# Provider Relay Identity Client Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将单体网页管理网关迁移为按 Windows 电脑与账户身份隔离的 Rust 服务端和 Tauri 桌面客户端，同时保留 `/v1/*` 协议桥接能力。

**Architecture:** 根 Cargo workspace 包含 `server`、`client/src-tauri` 和 `crates/protocol`。服务端持有 SQLite、加密后的供应商密钥、身份认证和协议桥接；Tauri 原生层持有设备凭据并通过 command 向 Nuxt 暴露业务操作；Nuxt 不直接请求服务端。`crates/protocol` 只共享管理 API DTO 与稳定错误码。

**Tech Stack:** Rust、Axum、SQLx SQLite、AES-256-GCM、Tauri 2、Nuxt 4、Tailwind 4、Bun、Windows Credential Manager、Bruno。

---

## 文件结构

```text
Cargo.toml
server/
  Cargo.toml
  Dockerfile
  build.rs
  src/{main.rs,app.rs,error.rs,identity/,storage/,routes/{management/,v1/},bridge/,providers/,observability/}
  tests/
client/
  package.json
  nuxt.config.ts
  app/{pages/,components/,composables/,stores/,utils/}
  src-tauri/{Cargo.toml,tauri.conf.json,capabilities/default.json,src/{lib.rs,api_client.rs,identity.rs,credential_store.rs,autostart.rs,tray.rs,commands/}}
crates/
  protocol/{Cargo.toml,src/{lib.rs,identity.rs,providers.rs,interfaces.rs,stats.rs,error.rs}}
docs/
  protocol/{bruno.json,environments/,management/,v1/}
docker/docker-compose.yml
```

根目录中现有 `src/`、`web/`、`build.rs`、`Dockerfile` 和根二进制 package 定义在迁移完成后不存在；根 `Cargo.toml` 作为 workspace manifest 保留。协议桥接代码只从旧 `src/bridge/`、`src/providers/`、`src/routes/{chat,responses,messages,models,interface_resolver}.rs` 移入 `server/src/` 对应职责目录；不得在迁移中改变现有协议转换规则。

### Task 1: 建立 Workspace 与共享管理协议 Crate

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/protocol/Cargo.toml`
- Create: `crates/protocol/src/lib.rs`
- Create: `crates/protocol/src/{identity,providers,interfaces,stats,error}.rs`
- Create: `crates/protocol/tests/management_dto.rs`

- [ ] **Step 1: 写入共享 DTO 的失败测试**

```rust
use provider_relay_protocol::{
    CreateIdentityRequest, CreateProviderRequest, InterfaceModelInput, ProtocolErrorCode,
};

#[test]
fn management_dtos_round_trip_without_identity_id_from_client() {
    let register = CreateIdentityRequest {
        machine_id: "machine-a".into(),
        account_sid: "S-1-5-21-100".into(),
    };
    let provider = CreateProviderRequest {
        name: "DeepSeek".into(),
        provider_type: "openai_compatible".into(),
        base_url: "https://api.deepseek.com".into(),
        api_key: "sk-test".into(),
        models: vec!["deepseek-chat".into()],
    };

    assert_eq!(serde_json::to_value(register).unwrap()["machine_id"], "machine-a");
    assert!(serde_json::to_value(provider).unwrap().get("identity_id").is_none());
    assert_eq!(InterfaceModelInput::default_model_name("upstream"), "upstream");
    assert_eq!(ProtocolErrorCode::IdentityAlreadyRegistered.as_str(), "identity_already_registered");
}
```

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p provider-relay-protocol --test management_dto`
Expected: 编译失败，因为 `provider-relay-protocol` crate 与这些 DTO 尚不存在。

- [ ] **Step 3: 建立 workspace 和 DTO**

根 `Cargo.toml` 只声明以下成员，不在根定义 package：

```toml
[workspace]
members = ["server", "client/src-tauri", "crates/protocol"]
resolver = "2"
```

`crates/protocol/src/lib.rs` 只重导出管理协议模块：

```rust
pub mod error;
pub mod identity;
pub mod interfaces;
pub mod providers;
pub mod stats;

pub use error::ProtocolErrorCode;
pub use identity::{CreateIdentityRequest, CreateIdentityResponse, RotateCredentialResponse};
pub use interfaces::{CreateInterfaceRequest, InterfaceModelInput, InterfaceResponse};
pub use providers::{CreateProviderRequest, ProviderResponse, UpdateProviderRequest};
```

所有 DTO 使用 `serde::{Deserialize, Serialize}`；只包含桌面客户端与 `/api/*` 之间的字段。`CreateIdentityRequest` 只包含 `machine_id`、`account_sid`，所有 Provider、Interface、统计 DTO 都不能包含客户端可赋值的 `identity_id`。

- [ ] **Step 4: 运行共享 crate 测试**

Run: `cargo test -p provider-relay-protocol --test management_dto`
Expected: PASS。

- [ ] **Step 5: 提交共享协议基础**

```text
git add Cargo.toml crates/protocol
git commit -m "建立共享管理协议 crate"
```

### Task 2: 迁移服务端目录并移除网页静态管理入口

**Files:**
- Create: `server/Cargo.toml`, `server/build.rs`, `server/Dockerfile`
- Create: `server/src/{main.rs,app.rs,error.rs}`
- Create: `server/src/{bridge/,providers/,routes/{management/,v1/},observability/}`
- Create: `server/tests/protocol_routes.rs`
- Delete: 根 `src/`, `build.rs`, `Dockerfile`, `web/`, `static/`
- Modify: `docker/docker-compose.yml`, `.dockerignore`, `README.md`

- [ ] **Step 1: 写入协议路由不依赖静态管理页的失败测试**

```rust
#[tokio::test]
async fn v1_models_route_is_registered_without_static_or_proxy_fallback() {
    let app = provider_relay_server::app::router(test_state()).await.unwrap();

    assert_eq!(status(&app, "/v1/models").await, StatusCode::UNAUTHORIZED);
    assert_eq!(status(&app, "/proxy").await, StatusCode::NOT_FOUND);
    assert_eq!(status(&app, "/").await, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p provider-relay-server --test protocol_routes v1_models_route_is_registered_without_static_or_proxy_fallback`
Expected: 编译失败，因为 `server` crate 和 `app::router` 尚不存在。

- [ ] **Step 3: 按职责移动现有服务端代码**

将旧 `src/bridge/` 和 `src/providers/` 原样迁入 `server/src/bridge/` 与 `server/src/providers/`。将 `/v1/models`、`/v1/responses`、`/v1/chat/completions`、`/v1/messages` 路由迁入 `server/src/routes/v1/`；将请求元数据、流式统计和统计查询迁入 `server/src/observability/`。

`server/src/app.rs` 只装配 `/api` 与 `/v1` 路由：

```rust
pub async fn router(state: AppState) -> Result<Router> {
    Ok(Router::new()
        .nest("/api", routes::management::router(state.clone()))
        .nest("/v1", routes::v1::router(state))
        .fallback(not_found))
}
```

Task 2 的 `routes/management/mod.rs` 只返回空 `Router`，不注册任何管理资源；Task 4 才新增经设备凭据认证的管理路由。这样目录迁移提交不存在网页、`/proxy` 或未认证的临时管理入口。该中间提交只用于代码迁移与测试，不能作为可部署版本。

不复制 `ServeDir`、`ServeFile`、`routes::proxy`、`ADMIN_TOKEN` 或 `CorsLayer::permissive()`。`server/build.rs` 不构建前端。

- [ ] **Step 4: 运行服务端路由测试**

Run: `cargo test -p provider-relay-server --test protocol_routes v1_models_route_is_registered_without_static_or_proxy_fallback`
Expected: PASS。

- [ ] **Step 5: 验证工作区和镜像边界**

Run: `cargo check -p provider-relay-server`
Expected: PASS，且构建不会调用 Bun。
Run: `rg -n "ServeDir|ServeFile|ADMIN_TOKEN|/proxy|web/" server Dockerfile docker`
Expected: 没有匹配；旧根 Dockerfile 已不存在。

- [ ] **Step 6: 提交服务端目录迁移**

```text
git add Cargo.toml server docker .dockerignore README.md
git rm -r src web static build.rs Dockerfile
git commit -m "拆分服务端并移除网页管理入口"
```

### Task 3: 实现身份存储、凭据哈希与供应商密钥加密

**Files:**
- Create: `server/src/identity/{mod.rs,credential.rs,cleanup.rs}`
- Create: `server/src/storage/{mod.rs,schema.rs,identities.rs,crypto.rs,providers.rs,interfaces.rs,sessions.rs,stats.rs}`
- Create: `server/tests/identity_storage.rs`
- Modify: `server/src/app.rs`, `server/src/error.rs`, `server/Cargo.toml`

- [ ] **Step 1: 写入身份定位、凭据和密钥密文失败测试**

```rust
#[tokio::test]
async fn identity_credentials_are_hashed_and_provider_keys_are_encrypted() {
    let storage = test_storage().await;
    let registered = storage.register_identity("machine-a", "S-1-5-21-100").await.unwrap();

    assert!(storage.authenticate_identity(&registered.credential).await.unwrap().is_some());
    assert_ne!(storage.identity_credential_hash(&registered.identity_id).await.unwrap(), registered.credential);

    let provider_id = storage.create_provider(&registered.identity_id, provider_input("sk-secret")).await.unwrap();
    assert_ne!(storage.raw_provider_key_ciphertext(&provider_id).await.unwrap(), "sk-secret");
    assert_eq!(storage.decrypt_provider_key(&provider_id).await.unwrap(), "sk-secret");
}

#[tokio::test]
async fn stable_key_cannot_reissue_a_lost_credential() {
    let storage = test_storage().await;
    storage.register_identity("machine-a", "S-1-5-21-100").await.unwrap();

    assert_eq!(
        storage.register_identity("machine-a", "S-1-5-21-100").await.unwrap_err().code(),
        ProtocolErrorCode::IdentityAlreadyRegistered,
    );
}
```

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p provider-relay-server --test identity_storage`
Expected: 编译失败，因为 identity storage API 尚不存在。

- [ ] **Step 3: 实现安全存储边界**

在 `identities` 中创建 `id`、`machine_id`、`account_sid`、`credential_hash`、`created_at` 与 `last_active_at`，并建立唯一索引：

```sql
CREATE TABLE identities (
  id TEXT PRIMARY KEY,
  machine_id TEXT NOT NULL,
  account_sid TEXT NOT NULL,
  credential_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_active_at TEXT NOT NULL,
  UNIQUE(machine_id, account_sid)
);
```

设备凭据用安全随机字节编码为 URL-safe 字符串；数据库只存 `SHA-256` 哈希。凭据是高熵随机值，哈希不承担口令派生职责。使用常量时间比较哈希。

`PROVIDER_RELAY_MASTER_KEY` 必须是 Base64 编码的 32 字节密钥。`storage/crypto.rs` 使用 AES-256-GCM，每次加密生成 96 位随机 nonce，并把 `nonce || ciphertext` Base64 保存到 `provider_configs.api_key_ciphertext`。启动时主密钥缺失、Base64 非法或长度不是 32 字节即失败，绝不回退为明文存储。

Provider、Interface、会话、请求日志和模型别名直接保存 `identity_id`。Provider 模型通过 Provider 归属，Interface 模型通过 Interface 归属，不重复存储可由父资源推导的身份字段。所有写入均在事务内建立归属关系。Provider API 读取 DTO 只暴露 `api_key_masked`。

- [ ] **Step 4: 运行身份与存储测试**

Run: `cargo test -p provider-relay-server --test identity_storage`
Expected: PASS。

- [ ] **Step 5: 运行严格 Rust 检查**

Run: `cargo fmt --all`
Expected: exit 0。
Run: `cargo clippy -p provider-relay-server --all-targets --all-features -- -D warnings`
Expected: PASS。

- [ ] **Step 6: 提交身份与密钥存储**

```text
git add server/Cargo.toml server/src/identity server/src/storage server/src/app.rs server/src/error.rs server/tests/identity_storage.rs
git commit -m "实现身份凭据与密钥加密存储"
```

### Task 4: 实现设备凭据认证和按身份限制的管理 API

**Files:**
- Create: `server/src/routes/management/{mod.rs,auth.rs,identities.rs,providers.rs,interfaces.rs,stats.rs}`
- Create: `server/tests/management_isolation.rs`
- Modify: `server/src/routes/v1/mod.rs`, `server/src/app.rs`, `server/src/error.rs`

- [ ] **Step 1: 写入跨身份访问失败测试**

```rust
#[tokio::test]
async fn management_credential_cannot_read_or_mutate_another_identity_provider() {
    let app = seeded_app().await;
    let (credential_a, provider_a) = create_identity_with_provider(&app, "machine-a", "S-1-5-21-100").await;
    let credential_b = register(&app, "machine-b", "S-1-5-21-200").await.credential;

    assert_eq!(get(&app, "/api/providers", &credential_b).await.json()[0]["id"], serde_json::Value::Null);
    assert_eq!(delete(&app, &format!("/api/providers/{provider_a}"), &credential_b).await.status(), StatusCode::NOT_FOUND);
    assert_eq!(get(&app, &format!("/api/providers/{provider_a}"), &credential_a).await.status(), StatusCode::OK);
}
```

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p provider-relay-server --test management_isolation management_credential_cannot_read_or_mutate_another_identity_provider`
Expected: FAIL，因为管理路由尚未从设备凭据注入身份范围。

- [ ] **Step 3: 实现管理认证与资源 API**

`POST /api/identities` 是唯一匿名管理路由。其余 `/api/*` 都由 `routes/management/auth.rs` 从 `Authorization: Bearer <device-credential>` 提取凭据、查找身份、刷新 `last_active_at`，并将 `CurrentIdentity { id }` 插入 request extensions。

管理资源函数统一接收 `CurrentIdentity`，例如：

```rust
async fn delete_provider(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<StatusCode, AppError> {
    state.storage.delete_provider(&identity.id, &provider_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
```

列表只查询 `WHERE identity_id = ?`；单资源读、改、删全部查询 `WHERE identity_id = ? AND id = ?`。跨身份资源一律返回 `404`。连通性测试、模型发现、Interface Token 重置和统计查询复用相同范围。`POST /api/identity/credential/rotate` 认证后原子更新凭据哈希，响应只返回新明文一次。

- [ ] **Step 4: 运行管理隔离测试**

Run: `cargo test -p provider-relay-server --test management_isolation`
Expected: PASS。

- [ ] **Step 5: 提交身份范围管理 API**

```text
git add server/src/routes/management server/src/routes/v1/mod.rs server/src/app.rs server/src/error.rs server/tests/management_isolation.rs
git commit -m "按身份限制管理接口"
```

### Task 5: 将 Interface Token 和 `/v1/*` 解析收敛到身份范围

**Files:**
- Modify: `server/src/routes/v1/{mod.rs,auth.rs,models.rs,chat.rs,responses.rs,messages.rs,interface_resolver.rs}`
- Modify: `server/src/storage/{interfaces.rs,providers.rs,sessions.rs,stats.rs}`
- Create: `server/tests/v1_identity_scope.rs`

- [ ] **Step 1: 写入同名模型不可跨身份解析的失败测试**

```rust
#[tokio::test]
async fn interface_token_resolves_only_its_identity_model_mapping() {
    let app = seeded_app().await;
    let interface_a = create_interface_for(&app, "machine-a", "S-1-5-21-100", "shared-model", "provider-a").await;
    create_interface_for(&app, "machine-b", "S-1-5-21-200", "shared-model", "provider-b").await;

    let response = post_v1_models(&app, &interface_a.token).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json()["data"][0]["id"], "shared-model");
    assert_eq!(resolved_provider(&app, &interface_a.token, "shared-model").await, "provider-a");
}
```

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p provider-relay-server --test v1_identity_scope interface_token_resolves_only_its_identity_model_mapping`
Expected: FAIL，因为现有 resolver 仅按 Interface 或 Provider 查询，未验证归属链。

- [ ] **Step 3: 修改协议认证和模型解析**

`routes/v1/auth.rs` 用 Interface Token 查询 Interface 与 `identity_id`，将 `CurrentProtocolAccess { identity_id, interface_id }` 放入 request extensions。`interface_resolver.rs` 的每个查询必须通过 Interface 的 `identity_id` 约束 Provider、Provider Model 和 Interface Model。

`/v1/models`、`/v1/responses`、`/v1/chat/completions`、`/v1/messages` 只读取 `CurrentProtocolAccess`，不能接受设备凭据。成功完成的协议请求刷新该 `identity_id` 的 `last_active_at`。请求日志和 Responses 会话写入相同 `identity_id`。

- [ ] **Step 4: 运行协议范围测试**

Run: `cargo test -p provider-relay-server --test v1_identity_scope`
Expected: PASS。

- [ ] **Step 5: 执行服务端回归**

Run: `cargo test -p provider-relay-server --all-targets --all-features`
Expected: PASS。

- [ ] **Step 6: 提交协议身份边界**

```text
git add server/src/routes/v1 server/src/storage server/tests/v1_identity_scope.rs
git commit -m "限制协议入口的身份模型解析"
```

### Task 6: 实现 90 天失活清理和不兼容数据迁移

**Files:**
- Modify: `server/src/identity/cleanup.rs`, `server/src/storage/{schema.rs,identities.rs}`
- Modify: `server/src/app.rs`, `server/src/main.rs`, `server/Cargo.toml`
- Create: `server/tests/identity_cleanup.rs`
- Modify: `server/Dockerfile`, `docker/docker-compose.yml`, `README.md`

- [ ] **Step 1: 写入失活身份级联删除的失败测试**

```rust
#[tokio::test]
async fn cleanup_removes_inactive_identity_and_all_owned_data() {
    let storage = test_storage().await;
    let identity = storage.register_identity("machine-a", "S-1-5-21-100").await.unwrap();
    seed_all_identity_resources(&storage, &identity.identity_id).await;
    storage.set_last_active_at(&identity.identity_id, "2026-01-01T00:00:00Z").await.unwrap();

    assert_eq!(storage.delete_inactive_identities(Utc::now(), Duration::days(90)).await.unwrap(), 1);
    assert!(!storage.identity_exists(&identity.identity_id).await.unwrap());
    assert_eq!(storage.count_owned_resources(&identity.identity_id).await.unwrap(), 0);
}
```

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p provider-relay-server --test identity_cleanup cleanup_removes_inactive_identity_and_all_owned_data`
Expected: 编译失败，因为清理 API 尚不存在。

- [ ] **Step 3: 实现清理和迁移策略**

服务启动后及每 24 小时执行一次 `delete_inactive_identities(now, Duration::days(90))`。删除在一个 SQLite transaction 中按顺序删除请求日志、会话、Interface 模型、Interface、Provider 模型、模型别名、Provider 和 identity；任一步失败回滚。

旧无身份数据库不做归属猜测。schema 初始化检测到旧 `provider_configs` 缺少 `identity_id` 时，在启动事务中按引用反向顺序删除旧业务表、删除旧 schema，并创建新 schema；操作记录 warning，但不导出、归档或自动分配旧密钥和配置。

Compose 不再传递 `ADMIN_TOKEN`，新增 `PROVIDER_RELAY_MASTER_KEY` 必填环境变量。Docker 镜像只复制服务端二进制与运行数据目录，不复制管理网页静态资源。

- [ ] **Step 4: 运行失活清理测试**

Run: `cargo test -p provider-relay-server --test identity_cleanup`
Expected: PASS。

- [ ] **Step 5: 提交生命周期与部署契约**

```text
git add server/src/identity server/src/storage server/src/app.rs server/src/main.rs server/Dockerfile docker/docker-compose.yml README.md server/tests/identity_cleanup.rs
git commit -m "清理失活身份并更新服务端部署"
```

### Task 7: 创建 Tauri 2、Nuxt 4 和 Tailwind 4 客户端基础

**Files:**
- Create: `client/package.json`, `client/nuxt.config.ts`, `client/app/app.vue`, `client/app/assets/css/main.css`
- Create: `client/src-tauri/{Cargo.toml,build.rs,tauri.conf.json,capabilities/default.json,src/lib.rs,src/main.rs}`
- Create: `client/tests/app-shell.test.ts`
- Modify: 根 `Cargo.toml`, `.gitignore`, `README.md`

- [ ] **Step 1: 写入客户端壳层失败测试**

```ts
import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

test('client uses Nuxt Tauri and Tailwind entrypoints', () => {
  const packageJson = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
  const config = readFileSync(new URL('../nuxt.config.ts', import.meta.url), 'utf8');

  expect(packageJson.scripts.typecheck).toBe('nuxt typecheck');
  expect(packageJson.devDependencies['@tauri-apps/cli']).toBeDefined();
  expect(config).toContain('@nuxtjs/tailwindcss');
});
```

- [ ] **Step 2: 运行失败测试**

Run: `cd client; bun test tests/app-shell.test.ts`
Expected: 失败，因为 `client/package.json` 尚不存在。

- [ ] **Step 3: 创建客户端工程**

使用 Bun 初始化 Nuxt 4 项目，并以 Tauri 2 scaffold 创建 `client/src-tauri`。客户端 package scripts 至少包括：

```json
{
  "dev": "nuxt dev",
  "build": "nuxt build",
  "typecheck": "nuxt typecheck",
  "test": "bun test",
  "tauri": "tauri"
}
```

`client/src-tauri/Cargo.toml` 将 `provider-relay-protocol` 作为 path dependency，`lib.rs` 仅注册 command、系统托盘和自启插件。Nuxt 运行时不得保存或读取服务端设备凭据。

- [ ] **Step 4: 运行客户端基础检查**

Run: `cd client; bun test tests/app-shell.test.ts`
Expected: PASS。
Run: `cd client; bun run typecheck`
Expected: PASS。

- [ ] **Step 5: 提交客户端 scaffold**

```text
git add Cargo.toml client .gitignore README.md
git commit -m "创建 Tauri Nuxt 桌面客户端"
```

### Task 8: 实现 Windows 身份、凭据库、自动启动和托盘

**Files:**
- Create: `client/src-tauri/src/{identity.rs,credential_store.rs,autostart.rs,tray.rs}`
- Create: `client/src-tauri/src/commands/{mod.rs,bootstrap.rs}`
- Create: `client/src-tauri/tests/bootstrap.rs`
- Modify: `client/src-tauri/{Cargo.toml,src/lib.rs,capabilities/default.json}`

- [ ] **Step 1: 写入原生启动失败测试**

```rust
#[test]
fn bootstrap_uses_windows_identity_and_never_exposes_credential() {
    let identity = FakeWindowsIdentity::new("machine-a", "S-1-5-21-100");
    let credentials = MemoryCredentialStore::with_secret("device-secret");
    let response = bootstrap(&identity, &credentials).unwrap();

    assert_eq!(response.machine_id, "machine-a");
    assert_eq!(response.account_sid, "S-1-5-21-100");
    assert!(serde_json::to_value(response).unwrap().get("device_credential").is_none());
}
```

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p provider-relay-client --test bootstrap`
Expected: 编译失败，因为 native identity 与 credential store 模块尚不存在。

- [ ] **Step 3: 实现原生系统集成**

`identity.rs` 通过 Windows API 读取机器标识和当前进程令牌的 SID，返回 `WindowsIdentity { machine_id, account_sid }`；用户名仅用于 UI 展示，不能作为注册或授权字段。为测试抽象 `IdentitySource` trait。

`credential_store.rs` 使用 Windows Credential Manager 保存、读取和删除设备凭据，服务名固定为 `provider-relay/device-credential`。为测试抽象 `CredentialStore` trait；Tauri command 和 Nuxt payload 均不返回凭据明文。

使用官方 Tauri 2 `tauri-plugin-autostart` 注册当前用户登录后启动。`tray.rs` 创建显示主窗口与退出应用菜单；窗口关闭时隐藏到托盘，只有显式退出才结束进程。

- [ ] **Step 4: 运行原生测试和格式检查**

Run: `cargo test -p provider-relay-client --test bootstrap`
Expected: PASS。
Run: `cargo fmt --all`
Expected: exit 0。
Run: `cargo clippy -p provider-relay-client --all-targets --all-features -- -D warnings`
Expected: PASS。

- [ ] **Step 5: 提交客户端系统集成**

```text
git add client/src-tauri
git commit -m "接入客户端身份凭据与托盘启动"
```

### Task 9: 实现客户端 API Client、Tauri Commands 与 Nuxt 管理视图

**Files:**
- Create: `client/src-tauri/src/{api_client.rs,commands/{providers.rs,interfaces.rs,stats.rs}}`
- Create: `client/app/{pages/index.vue,pages/providers.vue,pages/interfaces.vue,pages/stats.vue,pages/diagnostics.vue}`
- Create: `client/app/{components/providers/,components/interfaces/,components/stats/,composables/useRelayCommand.ts,stores/relay.ts,utils/errors.ts}`
- Create: `client/tests/{api-boundary,provider-flow,interface-flow,stats-flow}.test.ts`
- Modify: `client/src-tauri/src/{lib.rs,commands/mod.rs}`, `client/app/app.vue`

- [ ] **Step 1: 写入客户端只通过 command 管理服务端的失败测试**

```ts
import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

test('Nuxt provider view invokes a Tauri command instead of reading device credentials', () => {
  const view = readFileSync(new URL('../app/pages/providers.vue', import.meta.url), 'utf8');
  const api = readFileSync(new URL('../src-tauri/src/api_client.rs', import.meta.url), 'utf8');

  expect(view).toContain("invoke('providers_list')");
  expect(view).not.toContain('Authorization');
  expect(view).not.toContain('device-credential');
  expect(api).toContain('CredentialStore');
});
```

- [ ] **Step 2: 运行失败测试**

Run: `cd client; bun test tests/api-boundary.test.ts`
Expected: 失败，因为 Provider 页面、command 和 API client 尚不存在。

- [ ] **Step 3: 实现 API Client 和 Commands**

`api_client.rs` 是唯一构造对服务端请求的模块。它从 `CredentialStore` 读取设备凭据，在 `/api/*` 请求上添加 `Authorization: Bearer <credential>`，并把 `crates/protocol` DTO 序列化为 JSON。首次启动读取不到凭据时调用 `POST /api/identities`，立即保存返回凭据；已注册稳定键返回 `identity_already_registered` 时显示无法自动恢复的错误，不尝试重注册或伪造凭据。

Tauri command 的命名固定为：

```text
bootstrap
providers_list
providers_save
providers_delete
providers_ping
providers_discover_models
providers_test_protocol
interfaces_list
interfaces_save
interfaces_delete
interfaces_regenerate_token
stats_overview
stats_requests
stats_models
stats_providers
credential_rotate
```

命令只返回脱敏 DTO、操作状态与稳定错误码；不返回设备凭据或 Provider API Key。

- [ ] **Step 4: 实现 Nuxt 视图**

将现有网页管理台的用户功能迁入客户端页面：

- `providers.vue`：供应商、模型、密钥录入、模型发现、协议测试和连通性状态。
- `interfaces.vue`：Interface、模型映射、接口地址与 Interface Token 复制/重置。
- `stats.vue`：总览、模型和供应商聚合。
- `diagnostics.vue`：请求明细、错误、协议与延迟诊断。

页面只从 `useRelayCommand` 调用 Tauri command。密钥输入仅在保存命令中传递，保存后清空输入状态；列表与编辑回显均使用服务端返回的脱敏值。保留加载、失败、空数据和就绪状态，不使用网页 `localStorage` 管理认证或 Provider Token。

- [ ] **Step 5: 运行客户端功能测试**

Run: `cd client; bun test`
Expected: PASS。
Run: `cd client; bun run typecheck`
Expected: PASS。
Run: `cd client; bun run build`
Expected: PASS。

- [ ] **Step 6: 提交客户端管理替代**

```text
git add client
git commit -m "实现桌面端供应商接口与统计管理"
```

### Task 10: 补充协议验证材料、全量门禁与迁移文档

**Files:**
- Create: `docs/protocol/bruno.json`
- Create: `docs/protocol/environments/template.bru`
- Create: `docs/protocol/management/{register_identity,list_providers,create_provider,cross_identity_denied,rotate_credential}.bru`
- Create: `docs/protocol/v1/{models,responses,chat_completions,messages}.bru`
- Modify: `README.md`, `docs/protocol-bridge-gateway-design.md`, `docs/protocol-bridge-implementation-status.md`
- Delete: 旧网页管理台文档与旧 `/proxy` 调用示例

- [ ] **Step 1: 写入 Bruno 集合结构失败检查**

```powershell
$required = @(
  'docs/protocol/bruno.json',
  'docs/protocol/management/register_identity.bru',
  'docs/protocol/management/cross_identity_denied.bru',
  'docs/protocol/v1/models.bru'
)
$missing = $required | Where-Object { -not (Test-Path $_) }
if ($missing) { throw "Missing protocol verification files: $($missing -join ', ')" }
```

- [ ] **Step 2: 运行失败检查**

Run: 上述 PowerShell 脚本。
Expected: 失败，列出尚不存在的协议验证文件。

- [ ] **Step 3: 编写 Bruno 验证集合与部署文档**

`docs/protocol/bruno.json` 声明集合；`environments/template.bru` 只包含 `relay_url`、`device_credential`、`interface_token` 占位环境变量，不写入任何实际值。集合覆盖：首次注册、重复注册拒绝、当前身份 Provider CRUD、跨身份对象返回 `404`、凭据轮换旧凭据失效，以及四个 `/v1/*` 入口的 Interface Token 调用。

README 只描述服务端必需的 `PROVIDER_RELAY_MASTER_KEY`、端口和运行方式，以及客户端安装和使用 `/v1/*` 的方式。删除 `ADMIN_TOKEN`、网页管理台和 `/proxy` 描述。协议设计与实现状态文档同步身份隔离、桌面管理和 90 天删除边界。

- [ ] **Step 4: 运行 Bruno 集合结构检查**

Run: 上述 PowerShell 脚本。
Expected: PASS。

- [ ] **Step 5: 执行全量门禁**

Run: `cargo fmt --all`
Expected: exit 0。
Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: PASS。
Run: `cargo test --workspace --all-targets --all-features`
Expected: PASS。
Run: `cd client; bun test; bun run typecheck; bun run build`
Expected: PASS。
Run: `git diff --check`
Expected: exit 0。

真实供应商、真实 Windows Credential Manager、登录后自启、托盘行为、Docker 容器和 Bruno 请求需要在相应 Windows 或部署环境单独验收；它们不能由单元测试或静态检查替代。

- [ ] **Step 6: 提交协议材料与迁移文档**

```text
git add docs README.md
git commit -m "补充身份客户端协议验证材料"
```

## 计划自检

- 身份键、凭据认证、凭据丢失不重签、凭据轮换、90 天删除、密钥加密、身份范围、`/v1/*` 入口、`/proxy` 移除、Tauri 客户端、自启托盘和 Bruno 集合均有对应任务。
- `identity_id` 只由服务端从设备凭据确定；共享 DTO 与客户端页面均不包含可写归属字段。
- `machine_id + account_sid` 只用于首次身份定位；所有管理授权与跨身份检查都只依赖设备凭据。
- 所有 Rust 代码任务都要求 `cargo fmt` 和严格 Clippy；所有 Node.js 命令均使用 Bun。

# 本地设备凭据实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用跨平台应用数据文件替换 Windows Credential Manager，并使注册和凭据轮换在本地持久化、网络中断及响应丢失后都可安全重试。

**Architecture:** 客户端使用随机生成的设备凭据，并在请求服务端前原子写入应用数据目录的 JSON 文件。服务端仅保存其哈希，以 `machine_id + account_sid + credential_hash` 实现幂等注册。轮换期间本地文件同时保留旧凭据和待生效新凭据；普通请求优先尝试新凭据，只有收到 `401` 才回退旧凭据并清理待生效状态。

**Tech Stack:** Rust、Axum、SQLx SQLite、Tauri 2、Bun。新增客户端依赖只用于跨平台应用数据目录定位和原子文件覆盖；不使用 Windows Credential Manager、系统 Keychain 或加密 vault。

---

## 文件结构

```text
crates/protocol/src/identity.rs
crates/protocol/tests/management_dto.rs
server/src/storage/{mod.rs,identities.rs}
server/src/routes/management/identities.rs
server/tests/{identity_storage.rs,management_isolation.rs}
client/src-tauri/Cargo.toml
client/src-tauri/src/{api_client.rs,credential_store.rs,lib.rs,commands/mod.rs}
client/src-tauri/tests/{api_client.rs,credential_store.rs,management_command_contract.rs}
docs/superpowers/specs/2026-08-13-provider-relay-identity-client-design.md
```

`crates/protocol` 仍是 `/api/*` 管理 DTO 的唯一来源。`credential_store.rs` 只负责本地记录的读取与原子状态迁移；`api_client.rs` 只负责凭据生成、HTTP 重试和注册；`commands/mod.rs` 只编排轮换命令，不直接读写文件。

### Task 1: 让服务端接受客户端凭据并支持幂等注册

**Files:**
- Modify: `crates/protocol/src/{identity.rs,lib.rs}`
- Modify: `crates/protocol/tests/management_dto.rs`
- Modify: `server/src/storage/{mod.rs,identities.rs}`
- Modify: `server/src/routes/management/identities.rs`
- Modify: `server/tests/{identity_storage.rs,management_isolation.rs}`

- [ ] **Step 1: 写入注册与轮换 DTO 的失败测试**

在 `crates/protocol/tests/management_dto.rs` 添加：

```rust
#[test]
fn identity_credential_dtos_round_trip_without_server_issued_secret() {
    let request = CreateIdentityRequest {
        machine_id: "machine-a".into(),
        account_sid: "S-1-5-21-100".into(),
        credential: "client-generated-credential".into(),
    };
    let rotate = RotateCredentialRequest {
        new_credential: "next-client-credential".into(),
    };

    assert_eq!(serde_json::to_value(request).unwrap()["credential"], "client-generated-credential");
    assert_eq!(serde_json::to_value(rotate).unwrap()["new_credential"], "next-client-credential");
    assert!(serde_json::to_value(CreateIdentityResponse {
        identity_id: "identity-a".into(),
        created: false,
    }).unwrap().get("credential").is_none());
}
```

- [ ] **Step 2: 验证 DTO 红灯**

Run: `cargo test -p provider-relay-protocol --test management_dto identity_credential_dtos_round_trip_without_server_issued_secret`

Expected: FAIL，因为请求没有客户端凭据字段，响应和轮换请求仍包含服务端签发的凭据字段。

- [ ] **Step 3: 定义共享身份 DTO**

将 `CreateIdentityRequest` 改为包含 `machine_id`、`account_sid`、`credential`；将 `CreateIdentityResponse` 改为 `identity_id` 和 `created: bool`，绝不含凭据；新增 `RotateCredentialRequest { new_credential: String }`，将 `RotateCredentialResponse` 改为 `rotated: bool`。在 `lib.rs` 重导出新请求类型。

- [ ] **Step 4: 写入服务端幂等行为的失败测试**

在 `server/tests/identity_storage.rs` 添加：

```rust
#[tokio::test]
async fn registration_retries_only_when_the_client_credential_matches() {
    let storage = test_storage().await;

    let created = storage
        .register_identity("machine-a", "S-1-5-21-100", "credential-a")
        .await
        .unwrap();
    let retried = storage
        .register_identity("machine-a", "S-1-5-21-100", "credential-a")
        .await
        .unwrap();

    assert!(created.created);
    assert!(!retried.created);
    assert_eq!(created.identity_id, retried.identity_id);
    assert_eq!(
        storage
            .register_identity("machine-a", "S-1-5-21-100", "credential-b")
            .await
            .unwrap_err()
            .code(),
        ProtocolErrorCode::IdentityAlreadyRegistered,
    );
}
```

在 `server/tests/management_isolation.rs` 添加轮换请求体测试：旧凭据认证后提交 `new_credential`，新凭据可认证、旧凭据返回 `401`，响应 JSON 不含新凭据。

- [ ] **Step 5: 验证服务端红灯**

Run: `cargo test -p provider-relay-server --test identity_storage registration_retries_only_when_the_client_credential_matches`

Expected: FAIL，因为当前 `register_identity` 在服务端生成凭据并对重复稳定键直接报错。

- [ ] **Step 6: 实现幂等注册与显式轮换**

将 `identities::register` 和 `Storage::register_identity` 改为接收客户端凭据。插入成功时保存 `hash_credential(credential)` 并返回 `created: true`；唯一键冲突时查询现有 `credential_hash`，用既有常量时间比较函数验证哈希：相同则返回同一 `identity_id` 和 `created: false`，不同则返回 `IdentityAlreadyRegistered`。

将 `identities::rotate_credential` 和 `Storage::rotate_identity_credential` 改为接收 `new_credential`。更新 SQL 保留已有 `WHERE id = ? AND credential_hash = ?` 并将新值哈希写入；成功时返回 `{ rotated: true }`，不返回任何凭据明文。管理路由从 `RotateCredentialRequest` 读取新凭据，创建路由按 `created` 返回 `201` 或 `200`。

- [ ] **Step 7: 验证服务端绿灯**

Run: `cargo test -p provider-relay-protocol --test management_dto`

Expected: PASS。

Run: `cargo test -p provider-relay-server --test identity_storage`

Expected: PASS。

Run: `cargo test -p provider-relay-server --test management_isolation`

Expected: PASS。

- [ ] **Step 8: 格式、静态检查并提交**

Run: `cargo fmt --all`

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: 两个命令均 exit 0。

```text
git add crates/protocol/src/identity.rs crates/protocol/src/lib.rs crates/protocol/tests/management_dto.rs server/src/storage/mod.rs server/src/storage/identities.rs server/src/routes/management/identities.rs server/tests/identity_storage.rs server/tests/management_isolation.rs
git commit -m "支持客户端生成设备凭据"
```

### Task 2: 以应用数据文件实现本地凭据状态机

**Files:**
- Modify: `client/src-tauri/Cargo.toml`
- Modify: `client/src-tauri/src/{credential_store.rs,lib.rs}`
- Create: `client/src-tauri/tests/credential_store.rs`

- [ ] **Step 1: 写入本地记录原子状态迁移的失败测试**

在 `client/src-tauri/tests/credential_store.rs` 使用临时目录创建 `FileCredentialStore`，添加：

```rust
#[test]
fn file_store_keeps_current_credential_until_pending_rotation_is_confirmed() {
    let store = FileCredentialStore::at(tempdir().unwrap().path().join("device-credential.json"));

    store.save_initial("credential-old").unwrap();
    store.begin_rotation("credential-new").unwrap();
    assert_eq!(store.load().unwrap(), Some(CredentialRecord {
        current: "credential-old".into(),
        pending: Some("credential-new".into()),
    }));

    store.confirm_pending().unwrap();
    assert_eq!(store.load().unwrap().unwrap().current, "credential-new");
    assert!(store.load().unwrap().unwrap().pending.is_none());
}
```

另加测试验证 `discard_pending` 保留旧值，且每次写入后不存在临时文件、JSON 不含半截记录。

- [ ] **Step 2: 验证文件凭据库红灯**

Run: `cargo test -p provider-relay-client --test credential_store`

Expected: FAIL，因为 `FileCredentialStore`、`CredentialRecord` 和状态迁移 API 尚不存在。

- [ ] **Step 3: 实现跨平台本地文件凭据库**

在 `credential_store.rs` 以 `CredentialRecord { current: String, pending: Option<String> }` 替换单字符串存储模型。`CredentialStore` 提供 `load`、`save_initial`、`begin_rotation`、`confirm_pending`、`discard_pending` 与 `delete`；`MemoryCredentialStore` 保持相同语义供测试使用。

新增 `FileCredentialStore`。通过跨平台应用数据目录库定位 `Provider Relay/device-credential.json`，父目录不存在时创建。写入先落同目录临时文件、flush 后使用依赖提供的跨平台原子替换操作覆盖目标；序列化 JSON 仅包含 `current` 和可选 `pending`。读取空文件、非法 JSON 或空 `current` 返回稳定的 `credential_store_error`，不猜测或重签身份。

`NativeState` 使用 `FileCredentialStore`；删除 Windows Credential API 依赖、`WindowsCredentialStore`、Windows-only `windows` Cargo feature 与 `CREDENTIAL_TARGET`。`identity.rs` 仍只在 Windows 实现身份键读取，本任务不改变其语义。

- [ ] **Step 4: 验证文件凭据库绿灯**

Run: `cargo test -p provider-relay-client --test credential_store`

Expected: PASS。

Run: `cargo test -p provider-relay-client --test bootstrap`

Expected: PASS，且 bootstrap 继续只暴露凭据是否存在。

- [ ] **Step 5: 格式、静态检查并提交**

Run: `cargo fmt --all`

Run: `cargo clippy -p provider-relay-client --all-targets --all-features -- -D warnings`

Expected: 两个命令均 exit 0。

```text
git add client/src-tauri/Cargo.toml client/src-tauri/src/credential_store.rs client/src-tauri/src/lib.rs client/src-tauri/tests/credential_store.rs
git commit -m "改用本地文件保存设备凭据"
```

### Task 3: 实现客户端生成、注册重试和轮换恢复

**Files:**
- Modify: `client/src-tauri/src/{api_client.rs,commands/mod.rs}`
- Modify: `client/src-tauri/tests/{api_client.rs,management_command_contract.rs}`

- [ ] **Step 1: 写入注册重试和轮换恢复的失败测试**

在 `client/src-tauri/tests/api_client.rs` 添加：

```rust
#[tokio::test]
async fn registration_persists_client_credential_before_a_retryable_request() {
    let store = MemoryCredentialStore::default();
    let client = ApiClient::with_test_http(server_returning_connection_drop_after_accept(), &store);

    assert!(client.ensure_registered(&test_identity()).await.is_err());
    let credential = store.load().unwrap().unwrap().current;
    assert!(!credential.is_empty());

    client.ensure_registered(&test_identity()).await.unwrap();
    assert_eq!(server().last_registration_credential(), credential);
}

#[tokio::test]
async fn pending_rotation_falls_back_to_current_credential_after_new_credential_is_rejected() {
    let store = MemoryCredentialStore::with_record("credential-old", Some("credential-new"));
    let client = ApiClient::with_test_http(server_accepting_only("credential-old"), &store);

    client.get::<serde_json::Value>("/api/providers").await.unwrap();
    assert_eq!(store.load().unwrap().unwrap().current, "credential-old");
    assert!(store.load().unwrap().unwrap().pending.is_none());
}
```

另加测试覆盖：待生效凭据认证成功时提升为 current；网络错误时保留 pending；轮换命令先持久化 pending、使用旧凭据请求、成功后确认 pending；服务端响应不再包含 credential。

- [ ] **Step 2: 验证客户端红灯**

Run: `cargo test -p provider-relay-client --test api_client registration_persists_client_credential_before_a_retryable_request`

Expected: FAIL，因为当前客户端等待服务端签发凭据后才写入 Windows Credential Manager。

- [ ] **Step 3: 实现凭据生成和注册**

在 `api_client.rs` 用操作系统随机源生成至少 32 字节并使用 URL-safe Base64 编码。`ensure_registered` 在本地记录不存在时先调用 `save_initial`，再将同一凭据置入 `CreateIdentityRequest`。网络失败时保留本地凭据；下次启动提交相同请求，服务端按 Task 1 返回同一身份确认。

所有认证请求统一从 `CredentialRecord` 取凭据：有 `pending` 时先携带它；响应为 `401` 时以 `current` 重试一次。回退成功后执行 `discard_pending`；待生效凭据成功后执行 `confirm_pending`；网络、5xx 或非 `401` 的业务错误保留 pending，不改变本地状态。`authenticated_request` 仅返回首选凭据，不在该纯构造 API 中写入本地状态。

`credential_rotate` 生成新凭据、先调用 `begin_rotation`，然后显式以旧 `current` 凭据发送 `RotateCredentialRequest { new_credential }`。服务端成功后调用 `confirm_pending`；网络失败或响应丢失时不清理 pending，后续请求按回退规则恢复实际状态。

- [ ] **Step 4: 更新 command 契约测试**

在 `management_command_contract.rs` 断言：注册输入含客户端生成的 `credential`，轮换 command 只向服务端发送 `new_credential`，Nuxt invoke 响应及所有 command 输入均不包含 current/pending 凭据明文。

- [ ] **Step 5: 验证客户端绿灯**

Run: `cargo test -p provider-relay-client --test api_client`

Expected: PASS。

Run: `cargo test -p provider-relay-client --test management_command_contract`

Expected: PASS。

Run: `cargo test -p provider-relay-client --all-targets --all-features`

Expected: PASS。

- [ ] **Step 6: 格式、静态检查并提交**

Run: `cargo fmt --all`

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: 两个命令均 exit 0。

```text
git add client/src-tauri/src/api_client.rs client/src-tauri/src/commands/mod.rs client/src-tauri/tests/api_client.rs client/src-tauri/tests/management_command_contract.rs
git commit -m "支持本地凭据注册与轮换恢复"
```

### Task 4: 更新验证材料并执行全量门禁

**Files:**
- Create: `docs/protocol/management/register_identity.bru`
- Create: `docs/protocol/management/rotate_credential.bru`
- Create: `docs/protocol/environments/template.bru`
- Modify: `README.md`
- Modify: `docs/protocol-bridge-gateway-design.md`
- Modify: `docs/protocol-bridge-implementation-status.md`

- [ ] **Step 1: 写入 Bruno 请求结构失败检查**

```powershell
$register = Get-Content -Raw 'docs/protocol/management/register_identity.bru'
$rotate = Get-Content -Raw 'docs/protocol/management/rotate_credential.bru'
if ($register -notmatch '"credential"') { throw 'registration request must carry the client credential' }
if ($rotate -notmatch '"new_credential"') { throw 'rotation request must carry the next credential' }
if ($rotate -match 'credential"\s*:\s*"{{device_credential}}"') { throw 'rotation must not return or submit a server-issued credential' }
```

- [ ] **Step 2: 验证文档检查红灯**

Run: 上述 PowerShell 脚本。

Expected: FAIL，因为当前 Bruno 集合仍按服务端签发和返回凭据描述注册与轮换。

- [ ] **Step 3: 更新协议材料**

`register_identity.bru` 请求体使用 `machine_id`、`account_sid`、`credential`；环境模板将 `registration_credential` 标记为本地生成的占位值，不提供真实值。`rotate_credential.bru` 使用当前设备凭据作为 Authorization，并发送 `new_credential`，响应示例不含凭据。

README、协议设计和实现状态文档说明：设备凭据保存于本地应用数据文件；它不使用系统凭据库或 vault；注册对相同凭据幂等；轮换以 pending/current 文件状态恢复。删除所有 Windows Credential Manager 和服务端一次性签发凭据描述。

- [ ] **Step 4: 验证文档检查绿灯**

Run: 上述 PowerShell 脚本。

Expected: PASS。

- [ ] **Step 5: 执行全量门禁**

Run: `cargo fmt --all`

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Run: `cargo test --all-targets --all-features`

Run: `cd client; bun test; bun run typecheck; bun run generate`

Run: `git diff --check`

Expected: 所有命令 exit 0。Nuxt/Nitro 既有依赖打包警告可记录，但不得视为生成失败。

- [ ] **Step 6: 提交验证材料**

```text
git add docs/protocol README.md docs/protocol-bridge-gateway-design.md docs/protocol-bridge-implementation-status.md
git commit -m "更新本地设备凭据验证材料"
```

## 计划自检

- Task 1 将凭据生成责任移到客户端，并使同一稳定键和同一凭据哈希可幂等注册；不同凭据仍不能接管身份。
- Task 2 只替换凭据存储实现，明确保存 `current` 和 `pending` 的独立生命周期；不改变 Windows 身份键读取。
- Task 3 覆盖首次注册、响应丢失、轮换响应丢失、待生效凭据成功和失败回退，且 Nuxt 永不读取凭据明文。
- Task 4 同步 Bruno 与文档，并以 Rust 与 Bun 全量门禁收口。

# Provider Relay Consistency Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让供应商和接口配置以完整模型集合原子保存，修复父子资源边界，并收敛管理台状态与构建入口。

**Architecture:** Axum 创建和更新端点接收完整目标状态，`db.rs` 在单个 SQLx SQLite 事务中写入父记录和子集合；旧模型 CRUD 端点保留并复用相同约束。Vue 表单改为单请求提交，页面显式维护加载、失败、空数据和就绪状态。

**Tech Stack:** Rust、Axum、SQLx、SQLite、Vue 3、TypeScript、Axios、Bun、Vite

---

## 文件职责

- `src/models.rs`：管理 API 的完整保存 DTO。
- `src/db.rs`：Provider、Interface 和模型集合的事务边界。
- `src/routes/admin.rs`：输入校验、HTTP 状态和父子资源约束。
- `src/routes/interface_resolver.rs`：唯一的生产 Interface token 与模型解析路径。
- `src/routes/chat.rs`、`src/routes/messages.rs`、`src/routes/responses.rs`：使用真实 Interface fixture 的路由测试。
- `web/src/api/index.ts`：前端完整保存请求类型和 API 调用。
- `web/src/views/ProvidersView.vue`、`web/src/views/InterfacesView.vue`：单请求保存及四态加载界面。
- `web/src/utils/providers.ts`：Provider 表单规则；移除无读取方的 token 存储 API。
- `web/src/utils/apiErrors.ts`：把管理 API 错误转换为稳定的用户提示。
- `build.rs`、`web/package.json`、`.dockerignore`：可重复构建和质量入口。
- `README.md`、`docs/protocol-bridge-gateway-design.md`：运行前提和正式契约。

### Provider 原子保存

**Files:**
- Modify: `src/models.rs`
- Modify: `src/db.rs`
- Modify: `src/routes/admin.rs`
- Test: `src/db.rs`
- Test: `src/routes/admin.rs`

- [ ] **Step: 先写完整集合保存的失败测试**

在 `src/db.rs` 测试模块增加创建、替换和回滚断言，使用下列调用契约：

```rust
let provider = create_config_with_models(
    &db,
    "DeepSeek",
    "deepseek",
    "https://api.deepseek.com/v1",
    "sk-test",
    None,
    &["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
)
.await
.expect("create provider and models");

update_config_with_models(
    &db,
    &provider.id,
    Some("DeepSeek Updated"),
    None,
    None,
    None,
    None,
    Some(&["deepseek-chat".to_string()]),
)
.await
.expect("update provider and models");
```

同时在 `src/routes/admin.rs` 增加重复模型和空模型返回 `400` 的测试，请求 JSON 使用：

```json
{
  "name": "DeepSeek",
  "provider_type": "deepseek",
  "base_url": "https://api.deepseek.com/v1",
  "api_key": "sk-test",
  "models": ["deepseek-chat", " deepseek-chat "]
}
```

- [ ] **Step: 确认 RED**

Run: `cargo test db::tests::creates_and_replaces_provider_with_complete_model_set -- --exact`

Expected: FAIL，提示 `create_config_with_models` 尚不存在。

Run: `cargo test routes::admin::tests::rejects_duplicate_provider_models_in_atomic_save -- --exact`

Expected: FAIL，现有 DTO 不识别或不校验完整集合。

- [ ] **Step: 实现 Provider DTO 与事务函数**

在 `src/models.rs` 将请求定义为：

```rust
#[derive(Debug, Deserialize)]
pub struct CreateConfigRequest {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    pub capabilities: Option<ProviderCapabilityOverrides>,
    pub models: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub name: Option<String>,
    pub provider_type: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub capabilities: Option<ProviderCapabilityOverrides>,
    pub models: Option<Vec<String>>,
}
```

在 `src/db.rs` 增加 `create_config_with_models` 和 `update_config_with_models`。二者在 `pool.begin()` 后写父记录；替换时先删除不在目标集合中的 Interface 映射和 Provider 模型，再插入缺失模型，最后提交。模型记录已存在时保留原 ID 和 `created_at`。

在 `src/routes/admin.rs` 增加 `normalize_provider_models(models: Vec<String>) -> Result<Vec<String>, AppError>`，按 trim 后值检查空字符串和重复项，并让创建/更新端点返回带完整模型集合的 `ConfigResponse`。

- [ ] **Step: 确认 GREEN 与 Rust 格式**

Run: `cargo fmt --all`

Run: `cargo test db::tests::creates_and_replaces_provider_with_complete_model_set -- --exact`

Run: `cargo test routes::admin::tests::rejects_duplicate_provider_models_in_atomic_save -- --exact`

Expected: 目标测试全部 PASS。

- [ ] **Step: 提交 Provider 事务契约**

```text
git add src/models.rs src/db.rs src/routes/admin.rs
git commit -m "原子保存供应商模型集合"
```

### Interface 原子保存与父资源约束

**Files:**
- Modify: `src/models.rs`
- Modify: `src/db.rs`
- Modify: `src/routes/admin.rs`
- Test: `src/db.rs`
- Test: `src/routes/admin.rs`

- [ ] **Step: 先写 Interface 事务和跨父资源删除测试**

测试使用完整映射：

```rust
let models = vec![InterfaceModelInput {
    provider_id: provider.id.clone(),
    upstream_model: "deepseek-chat".to_string(),
    model_name: Some("coder".to_string()),
}];
let interface = create_interface_with_models(&db, "Main", "all", &models)
    .await
    .expect("create interface and mappings");
```

增加失败引用测试，传入不存在的 `upstream_model` 后断言 Interface 没有创建；增加两个 Interface 的模型后，调用：

```rust
let deleted = delete_interface_model(&db, &interface_a.id, &model_b.id)
    .await
    .expect("delete by parent and child id");
assert!(!deleted);
```

- [ ] **Step: 确认 RED**

Run: `cargo test db::tests::rolls_back_interface_when_model_reference_is_invalid -- --exact`

Expected: FAIL，提示完整保存函数尚不存在。

Run: `cargo test routes::admin::tests::does_not_delete_interface_model_from_another_interface -- --exact`

Expected: FAIL，当前实现只按 `model_id` 删除。

- [ ] **Step: 实现 Interface DTO、事务与删除条件**

在 `src/models.rs` 增加：

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct InterfaceModelInput {
    pub provider_id: String,
    pub upstream_model: String,
    pub model_name: Option<String>,
}
```

`CreateInterfaceRequest.models` 为 `Vec<InterfaceModelInput>`，`UpdateInterfaceRequest.models` 为 `Option<Vec<InterfaceModelInput>>`。在路由层规范化三个字段，拒绝空值和重复 `model_name`。

在 `src/db.rs` 增加 `create_interface_with_models`、`update_interface_with_models` 和事务内的引用检查。更新时只有 `models = Some(...)` 才替换集合；删除函数签名改为：

```rust
pub async fn delete_interface_model(
    pool: &SqlitePool,
    interface_id: &str,
    model_id: &str,
) -> Result<bool>
```

SQL 条件使用 `WHERE id = ? AND interface_id = ?`，路由提取参数名改为 `:interface_id`。

- [ ] **Step: 确认 GREEN 与相关回归**

Run: `cargo fmt --all`

Run: `cargo test db::tests::rolls_back_interface_when_model_reference_is_invalid -- --exact`

Run: `cargo test routes::admin::tests::does_not_delete_interface_model_from_another_interface -- --exact`

Expected: 目标测试全部 PASS。

Run: `cargo test routes::admin::tests`

Expected: admin 路由测试全部 PASS。

- [ ] **Step: 提交 Interface 事务契约**

```text
git add src/models.rs src/db.rs src/routes/admin.rs
git commit -m "原子保存接口模型映射"
```

### 生产解析链测试收敛

**Files:**
- Modify: `src/routes/interface_resolver.rs`
- Modify: `src/routes/chat.rs`
- Modify: `src/routes/messages.rs`
- Modify: `src/routes/responses.rs`
- Test: `src/routes/interface_resolver.rs`
- Test: `src/routes/chat.rs`
- Test: `src/routes/messages.rs`
- Test: `src/routes/responses.rs`

- [ ] **Step: 写没有 Interface 上下文时拒绝解析的测试**

```rust
#[tokio::test]
async fn rejects_model_resolution_without_authenticated_interface() {
    let db = test_db().await;
    let headers = HeaderMap::new();
    let error = resolve_interface_model(&db, &headers, "deepseek-chat")
        .await
        .expect_err("missing interface must be rejected");
    assert!(matches!(error, AppError::Unauthorized(_)));
}
```

为三种协议的成功路由测试准备同一类 fixture：创建 Provider、Provider model、Interface 和 Interface model，在 HeaderMap 中写入生产中间件使用的 Interface 扩展或真实 Bearer token。

- [ ] **Step: 确认 RED**

Run: `cargo test routes::interface_resolver::tests::rejects_model_resolution_without_authenticated_interface -- --exact`

Expected: FAIL，测试构建下仍会走旧 Provider/alias 回退。

- [ ] **Step: 删除测试回退并更新 fixture**

移除 `resolve_interface_model` 中整个 `#[cfg(test)]` 旧路径。路由测试统一通过辅助函数创建：

```rust
async fn create_routable_model(
    db: &SqlitePool,
    downstream_protocol: &str,
    downstream_model: &str,
) -> (ProviderConfig, InterfaceConfig, HeaderMap)
```

辅助函数必须写入 Provider model 和 Interface model，不得调用旧 `model_aliases` 作为成功路径。

- [ ] **Step: 确认 GREEN 与三协议回归**

Run: `cargo fmt --all`

Run: `cargo test routes::interface_resolver::tests`

Run: `cargo test routes::chat::tests`

Run: `cargo test routes::messages::tests`

Run: `cargo test routes::responses::tests`

Expected: 所有目标测试 PASS，源码中 `rg -n "cfg\(test\)" src/routes/interface_resolver.rs` 只命中测试模块声明。

- [ ] **Step: 提交生产解析测试路径**

```text
git add src/routes/interface_resolver.rs src/routes/chat.rs src/routes/messages.rs src/routes/responses.rs
git commit -m "统一测试与生产接口解析链"
```

### Provider 管理台单请求保存与错误状态

**Files:**
- Create: `web/src/utils/apiErrors.ts`
- Modify: `web/src/api/index.ts`
- Modify: `web/src/utils/providers.ts`
- Modify: `web/src/views/ProvidersView.vue`
- Test: `web/tests/provider-models.test.ts`
- Test: `web/tests/provider-form-layout.test.ts`
- Create: `web/tests/api-errors.test.ts`

- [ ] **Step: 先写前端契约失败测试**

```ts
test('provider save request carries the complete normalized model set', () => {
  expect(providerSaveModels([' a ', '', 'a', 'b'])).toEqual(['a', 'b']);
});

test('management auth failures have a dedicated message', () => {
  expect(managementErrorMessage({ response: { status: 401 } })).toBe(
    '管理凭据无效或缺失，请检查 ADMIN_TOKEN。',
  );
});
```

在源码契约测试中断言 `saveProvider` 只调用一次 `configApi.create/update`，请求包含 `models`，且源码不再包含 `createProviderModel`、`deleteProviderModel`、`saveToken`。

- [ ] **Step: 确认 RED**

Run: `bun test tests/provider-models.test.ts tests/provider-form-layout.test.ts tests/api-errors.test.ts`

Working directory: `web`

Expected: FAIL，错误转换函数和单请求保存尚不存在。

- [ ] **Step: 实现 API 类型、单请求保存和四态加载**

`CreateConfigRequest.models` 定义为 `string[]`，`UpdateConfigRequest.models` 定义为可选 `string[]`。`ProvidersView.vue` 在一次请求中发送 `normalizeProviderModelNames(form.models)`；移除模型差集函数和 `saveToken` 调用。

新增：

```ts
export function managementErrorMessage(error: unknown): string {
  if (axios.isAxiosError(error) && error.response?.status === 401) {
    return '管理凭据无效或缺失，请检查 ADMIN_TOKEN。';
  }
  return axios.isAxiosError(error) && !error.response
    ? '无法连接管理服务，请检查服务状态后重试。'
    : '加载失败，请稍后重试。';
}
```

页面使用 `loading` 和 `loadError` 区分加载中、失败、空数据和表格；失败区域提供调用 `loadData` 的“重试”按钮。模块加载时执行 `localStorage.removeItem('provider-relay-tokens')`，并从 `providers.ts` 删除 `StoredToken`、`getStoredTokens`、`saveToken`、`removeStoredToken`、`updateStoredToken`、`replaceStoredToken`。

- [ ] **Step: 确认 GREEN**

Run: `bun test tests/provider-models.test.ts tests/provider-form-layout.test.ts tests/api-errors.test.ts`

Working directory: `web`

Expected: 目标测试全部 PASS。

- [ ] **Step: 提交 Provider 管理台改动**

```text
git add web/src/api/index.ts web/src/utils/apiErrors.ts web/src/utils/providers.ts web/src/views/ProvidersView.vue web/tests/provider-models.test.ts web/tests/provider-form-layout.test.ts web/tests/api-errors.test.ts
git commit -m "收敛供应商管理台保存与错误状态"
```

### Interface 管理台单请求保存与错误状态

**Files:**
- Modify: `web/src/api/index.ts`
- Modify: `web/src/views/InterfacesView.vue`
- Test: `web/tests/interface-layout.test.ts`

- [ ] **Step: 先写 Interface 单请求保存失败测试**

```ts
test('interface editor sends the complete model mapping in one request', () => {
  const saveStart = source.indexOf('async function saveInterface');
  const saveEnd = source.indexOf('\nfunction ', saveStart + 1);
  const saveBody = source.slice(saveStart, saveEnd);
  expect(saveBody).toContain('models: form.value.models.map');
  expect(saveBody).not.toContain('createInterfaceModel');
  expect(saveBody).not.toContain('deleteInterfaceModel');
});
```

另加加载失败区含 `loadError`、`managementErrorMessage` 和 `@click="loadData"` 的断言。

- [ ] **Step: 确认 RED**

Run: `bun test tests/interface-layout.test.ts`

Working directory: `web`

Expected: FAIL，现有页面仍编排模型 CRUD。

- [ ] **Step: 实现完整映射请求和四态加载**

在 `web/src/api/index.ts` 增加 `InterfaceModelInput`，并让创建/更新请求携带：

```ts
models: form.value.models.map((model) => ({
  provider_id: model.provider_id,
  upstream_model: model.upstream_model,
  model_name: model.model_name,
}))
```

删除 `syncInterfaceModels` 和 `createMissingInterfaceModels`。页面复用 `managementErrorMessage`，保存失败保留抽屉内容，加载失败显示重试，成功空集合才显示空态。

- [ ] **Step: 确认 GREEN**

Run: `bun test tests/interface-layout.test.ts`

Working directory: `web`

Expected: 目标测试全部 PASS。

- [ ] **Step: 提交 Interface 管理台改动**

```text
git add web/src/api/index.ts web/src/views/InterfacesView.vue web/tests/interface-layout.test.ts
git commit -m "收敛接口管理台保存与错误状态"
```

### 构建、Docker 与正式文档

**Files:**
- Modify: `build.rs`
- Modify: `web/package.json`
- Modify: `.dockerignore`
- Create: `README.md`
- Modify: `docs/protocol-bridge-gateway-design.md`
- Modify: `docs/protocol-bridge-implementation-status.md`
- Test: `web/tests/provider-form-layout.test.ts`

- [ ] **Step: 先写构建入口失败测试**

在 `provider-form-layout.test.ts` 解析 `package.json` 并断言：

```ts
const packageJson = JSON.parse(packageSource);
expect(packageJson.scripts.test).toBe('bun test');
expect(packageJson.scripts.typecheck).toBe('vue-tsc --noEmit');
expect(packageJson.scripts.build).toBe('bun run typecheck && vite build');
```

增加 `build.rs` 源码断言：包含 `web/bun.lock`、不包含 `bun install`、缺失 `node_modules` 时包含 `bun install --frozen-lockfile` 提示。

- [ ] **Step: 确认 RED**

Run: `bun test tests/provider-form-layout.test.ts`

Working directory: `web`

Expected: FAIL，当前脚本缺少 `test`、`typecheck`，`build.rs` 仍隐式安装。

- [ ] **Step: 实现可重复构建入口**

`build.rs` 增加：

```rust
println!("cargo:rerun-if-changed=web/bun.lock");
if !Path::new("web/node_modules").exists() {
    panic!("web/node_modules is missing; run `cd web` then `bun install --frozen-lockfile`");
}
```

删除安装命令。`package.json` 增加 `"test": "bun test"`、`"typecheck": "vue-tsc --noEmit"`，`build` 改为 `bun run typecheck && vite build`。

`.dockerignore` 加入 `.env`、`.env.*`、`web/.env`、`web/.env.*`。README 记录冻结安装、运行环境变量、受控环境鉴权边界和验证命令；正式设计与状态文档记录 Interface token、完整模型集合和事务保存，不使用阶段编号组织正式设计内容。

- [ ] **Step: 确认 GREEN 和文档边界**

Run: `bun test tests/provider-form-layout.test.ts`

Working directory: `web`

Expected: 目标测试全部 PASS。

Run: `rg -n "ADMIN_TOKEN|bun install --frozen-lockfile|Interface token|完整模型集合" README.md docs/protocol-bridge-gateway-design.md docs/protocol-bridge-implementation-status.md`

Expected: 每项契约都有明确命中。

- [ ] **Step: 提交构建和文档**

```text
git add build.rs web/package.json .dockerignore README.md docs/protocol-bridge-gateway-design.md docs/protocol-bridge-implementation-status.md web/tests/provider-form-layout.test.ts
git commit -m "补齐可重复构建与运行文档"
```

### 全量验证与交付审查

**Files:**
- Review: all files changed by this plan

- [ ] **Step: 执行 Rust 格式、静态检查和测试**

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Expected: 命令退出码均为 0；若并行测试再次出现偶发失败，保留原始失败输出，并分别执行单测、串行全套和再次并行全套，不把复跑通过当成根因已解决。

- [ ] **Step: 执行前端质量入口**

```text
cd web
bun test
bun run format:check
bun run lint
bun run typecheck
bun run build
```

Expected: 命令退出码均为 0，测试失败数为 0。

- [ ] **Step: 检查工作树与范围**

```text
git diff --check
git status --short --branch
git log --format="%h %s" -n 10
```

Expected: `git diff --check` 退出码为 0；状态中不存在被遗漏的本计划改动，未启动 dev server，未改写 `web/bun.lock` 的依赖源。

- [ ] **Step: 两阶段审查**

先按设计逐条核对 Provider/Interface 原子性、旧 API 兼容、父子删除、生产解析链、管理台四态和构建入口；再做代码质量审查，检查事务错误传播、重复校验、前端未处理 Promise、死代码和测试真实性。所有 Critical/Important 问题修复并重新执行对应验证后才能收口。

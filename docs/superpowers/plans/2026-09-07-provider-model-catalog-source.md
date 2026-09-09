# 供应商模型目录唯一来源实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除 `identity_provider_models` 及其协议副本，让 `ProviderCatalog` 成为供应商模型唯一来源，同时保留接入点模型映射和多供应商路由。

**Architecture:** `identity_provider_configs.provider_type` 关联部署目录中的供应商和模型集合；服务端校验与客户端选项都从 `ProviderCatalog` 推导。`identity_endpoint_models` 继续保存用户显式配置的接入点模型映射，目录新增模型不会自动加入接入点。

**Tech Stack:** Rust, Axum, SeaORM/SeaQuery, PostgreSQL, SQLite, shared Rust protocol crate, Tauri, Nuxt/Vue, Bun/Vitest.

**Spec:** `prelay-server/docs/superpowers/specs/2026-09-07-provider-model-catalog-source-design.md`

## Global Constraints

- `prelay-protocol` 是管理 API DTO 的唯一来源；修改后再更新 `prelay-server/crates/protocol` submodule。
- `identity_provider_configs` 只保存连接配置；Provider API Key 仍只保存加密密文。
- `identity_endpoint_models` 和 `identity_endpoint_model_routes` 保持，目录新增模型不自动加入既有接入点。
- 正式调用入口保持 `/v1/models`、`/v1/responses`、`/v1/chat/completions`、`/v1/messages`。
- 不回退、覆盖或提交 `prelay-client` 中已有的 `config/catalog/models/language.toml`、`app/components/agents/CodexSettingsForm.vue`、`app/utils/modelReasoning.ts`、`tests/agent-settings-reasoning.test.ts`、`tests/model-reasoning.test.ts` 改动。
- Rust 修改完成后执行 `cargo fmt --all` 和 `cargo clippy --all-targets --all-features -- -D warnings`；Node 项目只使用 Bun。
- 未经用户另行授权，不推送、发布、打标签或修改远端历史。

---

### Task 1: 协议 DTO 删除供应商模型副本

**Files:**
- Modify: `prelay-protocol/src/providers.rs`
- Modify: `prelay-protocol/src/lib.rs`
- Test: `prelay-protocol` 现有管理 DTO 测试与新增供应商响应断言
- Update submodule working tree: `prelay-server/crates/protocol`

**Interfaces:**
- Consumes: 现有 `ProviderCatalogResponse`、`CatalogProviderResponse`、`CreateProviderRequest`、`UpdateProviderRequest`。
- Produces: `ProviderResponse` 只包含供应商连接信息和能力字段；创建/更新供应商请求不再包含 `models`；删除仅服务于供应商模型数据库行的 `ProviderModelResponse`。

- [ ] **Step 1: Write the failing protocol tests**

  在 `prelay-protocol` 的管理 DTO 测试中构造不带 `models` 的 `CreateProviderRequest`、`UpdateProviderRequest` 和 `ProviderResponse`，断言序列化 JSON 不包含 `models`，并删除原先依赖 `ProviderModelResponse` 的构造。

- [ ] **Step 2: Run the focused protocol tests and verify failure**

  Run:

  ```text
  cargo test --manifest-path prelay-protocol/Cargo.toml
  ```

  Expected: FAIL because现有 DTO 仍要求 `models` 字段，且 `ProviderResponse` 仍暴露供应商模型列表。

- [ ] **Step 3: Remove the duplicate protocol fields**

  从 `prelay-protocol/src/providers.rs` 删除：

  ```rust
  pub models: Vec<String>,
  pub models: Option<Vec<String>>,
  pub struct ProviderModelResponse,
  pub models: Vec<ProviderModelResponse>,
  ```

  从 `prelay-protocol/src/lib.rs` 删除 `ProviderModelResponse` 导出。保留目录供应商的 `language_models` 和 `image_generation_models`，因为它们属于 `ProviderCatalogResponse` 的目录关系。

- [ ] **Step 4: Run the focused protocol tests and verify pass**

  Run:

  ```text
  cargo test --manifest-path prelay-protocol/Cargo.toml
  cargo fmt --manifest-path prelay-protocol/Cargo.toml --all
  ```

  Expected: protocol tests PASS and formatting completes.

- [ ] **Step 5: Refresh the server protocol submodule checkout**

  将 `prelay-server/crates/protocol` 更新到包含协议变更的 `prelay-protocol` commit；检查 `git -C prelay-protocol diff`、`git -C prelay-server diff --submodule=short`，不提交任何仓库。

### Task 2: 服务端按目录推导供应商模型

**Files:**
- Modify: `prelay-server/src/storage/providers.rs`
- Modify: `prelay-server/src/storage/provider_views.rs`
- Modify: `prelay-server/src/storage/provider_validation.rs`
- Modify: `prelay-server/src/storage/endpoints.rs`
- Modify: `prelay-server/src/storage/endpoint_validation.rs`
- Modify: `prelay-server/src/storage/access.rs`
- Modify: `prelay-server/src/routes/v1/endpoint_resolver.rs`
- Modify: `prelay-server/src/routes/v1/models.rs`
- Modify: `prelay-server/src/entity/identity/mod.rs`
- Delete: `prelay-server/src/entity/identity/provider_models.rs`
- Modify: `prelay-server/src/schema/mod.rs`
- Modify: `prelay-server/src/schema/tables/providers.rs`
- Modify: `prelay-server/src/schema/indexes.rs`
- Test: `prelay-server/tests/management/providers.rs`
- Test: `prelay-server/tests/management/endpoints.rs`
- Test: `prelay-server/tests/identity/storage/candidates.rs`
- Test: `prelay-server/tests/v1/identity_scope.rs`

**Interfaces:**
- Consumes: Task 1 的 `ProviderResponse`、`CreateProviderRequest`、`UpdateProviderRequest` 和现有 `ProviderCatalog` 查询接口。
- Produces: 供应商 CRUD 不读写模型表；接入点保存和运行时模型解析通过 `ProviderCatalog` 校验；`ProviderResponse` 不再包含模型集合。

- [ ] **Step 1: Add failing behavior tests**

  增加以下测试观察面：

  - 已存在的供应商在目录包含模型时，`GET /api/providers` 成功返回连接资源，不需要供应商模型行。
  - 新建接入点时，`upstream_model` 只要属于供应商 `provider_type` 的目录关系即可通过。
  - 删除旧供应商模型表后，正式模型列表仍只返回有效的 `identity_endpoint_models`。
  - 目录移除模型后，旧接入点映射不会进入正式模型列表或解析候选。

- [ ] **Step 2: Run focused server tests and verify failure**

  Run:

  ```text
  cargo test --manifest-path prelay-server/Cargo.toml --test management providers endpoints
  cargo test --manifest-path prelay-server/Cargo.toml --test v1 identity_scope
  ```

  Expected: FAIL at DTO construction and current `identity_provider_models` lookup paths.

- [ ] **Step 3: Remove provider model persistence from provider storage**

  在 `prelay-server/src/storage/providers.rs`：

  - 删除 `provider_models` 实体导入。
  - 删除创建供应商后的 `insert_models`。
  - 删除更新供应商时按模型数组删除并重建行的逻辑。
  - 删除 `add_provider_models`、`add_models` 和 `insert_models`。
  - 保留供应商连接配置创建、更新、删除和 API Key 加密。
  - 创建/更新时继续使用 `ProviderCatalog` 校验 `provider_type`，但不再校验请求模型数组。

- [ ] **Step 4: Derive provider model options from the catalog**

  在 `prelay-server/src/storage/provider_views.rs` 删除对 `identity_provider_models` 的查询和 `provider_model_response`，让 `ProviderResponse` 只组装配置字段。

  将供应商模型显示与数量责任移到客户端目录索引，服务端不生成没有持久化身份的模型行。

- [ ] **Step 5: Replace endpoint validation with catalog validation**

  将 `endpoint_validation::validate_models` 改为接收 `&ProviderCatalog`，保留 identity 范围校验，并改用：

  ```rust
  catalog.provider_supports_language_model(&provider.provider_type, &model.upstream_model)
      || catalog.provider_supports_image_generation_model(
          &provider.provider_type,
          &model.upstream_model,
      )
  ```

  删除 `identity_provider_models` 查询。所有实际管理路由使用带目录的 endpoint storage 方法；测试辅助代码改为加载测试目录。

- [ ] **Step 6: Make runtime access catalog-aware**

  将 `Storage::select_protocol_model_candidates`、`Storage::list_protocol_models` 及其调用链增加 `&ProviderCatalog` 参数，删除 `provider_models` 查询，改为按已加载供应商的 `provider_type` 检查目录关系。

  在 `endpoint_resolver.rs` 和 `routes/v1/models.rs` 传入 `state.provider_catalog`。保留接入点模型表的候选顺序、路由选择和 identity 隔离。

- [ ] **Step 7: Remove the schema and entity registration**

  从 schema 初始化中删除 `identity_provider_models` 的建表和 `ProviderModels` 索引，从 identity entity module 删除模块注册并删除实体文件。同步更新 schema contract/initialization tests 的预期表集合。

- [ ] **Step 8: Run focused server tests and verify pass**

  Run:

  ```text
  cargo test --manifest-path prelay-server/Cargo.toml --test management providers endpoints
  cargo test --manifest-path prelay-server/Cargo.toml --test v1 identity_scope
  cargo test --manifest-path prelay-server/Cargo.toml --test identity storage
  ```

  Expected: all focused tests PASS, and no test uses a provider model table.

### Task 3: 已有数据库迁移删除旧模型副本

**Files:**
- Modify: `prelay-server/src/schema/provider_catalog.rs`
- Modify: `prelay-server/src/schema/mod.rs`
- Modify: `prelay-server/tests/schema/initialization.rs`
- Modify: `prelay-server/tests/schema/contract.rs`
- Add or modify: `prelay-server/tests/schema/provider_catalog.rs`

**Interfaces:**
- Consumes: Task 2 的目录校验和接入点运行时模型语义。
- Produces: PostgreSQL 可幂等执行的目录迁移，删除 `identity_provider_models`，保留有效接入点映射并清理目录外映射；SQLite 每次使用全新 schema，不执行旧数据迁移。

- [ ] **Step 1: Add failing migration tests**

  为 PostgreSQL migration test 建立包含以下表和数据的 fixture：

  - 一个目录内供应商和一个目录外模型；
  - 一个有效接入点模型；
  - 一个目录外接入点模型及其 route；
  - 一个目录外 model alias；
  - `identity_provider_models` 表及模型行。

  断言迁移后旧表不存在、有效接入点模型保留、目录外接入点模型/route/alias 被删除，重复执行不改变结果。

- [ ] **Step 2: Run the migration tests and verify failure**

  Run:

  ```text
  cargo test --manifest-path prelay-server/Cargo.toml schema::provider_catalog
  ```

  Expected: FAIL because当前迁移尚未删除旧表。

- [ ] **Step 3: Implement the versioned PostgreSQL migration**

  增加新的迁移版本，例如 `provider_catalog_v2`，执行顺序固定为：

  1. 读取供应商配置并将旧 `provider_type` 映射到目录供应商。
  2. 按目录关系清理 `identity_endpoint_models`、`identity_endpoint_model_routes` 和 `identity_model_aliases`。
  3. `DROP TABLE IF EXISTS identity_provider_models`。
  4. 写入迁移版本。

  使用 PostgreSQL 参数占位符和事务；未知供应商类型或无法映射的供应商返回迁移错误并回滚。SQLite 不执行这条迁移。

- [ ] **Step 4: Update schema contract tests**

  将 schema 初始化测试改为确认基础 schema 不再创建 `identity_provider_models`，并增加迁移后旧表删除、迁移幂等和目录外数据清理断言。

- [ ] **Step 5: Run schema and migration verification**

  Run:

  ```text
  cargo test --manifest-path prelay-server/Cargo.toml --test schema
  cargo test --manifest-path prelay-server/Cargo.toml schema::provider_catalog
  ```

  Expected: SQLite schema tests PASS。PostgreSQL migration behavior在可用测试库上验证；没有测试库时保留该验证缺口，不用 SQLite 替代。

### Task 4: 客户端从目录获取供应商模型

**Files:**
- Modify: `prelay-client/app/stores/relay.ts`
- Modify: `prelay-client/app/utils/endpointModels.ts`
- Modify: `prelay-client/app/composables/useProviderForm.ts`
- Modify: `prelay-client/app/components/providers/ProviderForm.vue`
- Modify: `prelay-client/app/components/providers/ProviderList.vue`
- Modify: `prelay-client/app/components/endpoints/EndpointForm.vue`
- Modify: `prelay-client/app/pages/providers.vue`
- Modify: `prelay-client/src-tauri/src/commands/providers.rs`
- Modify: `prelay-client/tests/provider-flow.test.ts`
- Modify: `prelay-client/tests/endpoint-flow.test.ts`
- Modify: `prelay-client/tests/model-catalog.test.ts`
- Modify: `prelay-client/tests/provider-operation-result.test.ts`

**Interfaces:**
- Consumes: Task 1 的无 `models` 供应商 DTO 和现有全局 `ProviderCatalogResponse`。
- Produces: 客户端 `Provider` 只保存连接配置；供应商模型数量和接入点模型选项按 `provider_type` 查询目录；供应商保存命令不发送模型数组。

- [ ] **Step 1: Add failing client tests**

  增加断言：

  - `Provider` 类型不包含 `models`。
  - `ProviderSaveInput` 和 `providers_save` payload 不包含 `models`。
  - 已有供应商表单按 `provider_type` 显示目录中的全部模型，不读取已保存模型数组。
  - 接入点可从 `modelCatalogProviderModels(provider.provider_type)` 获得上游模型选项。
  - 供应商列表模型数量来自目录，不来自 `row.models.length`。

- [ ] **Step 2: Run focused Bun tests and verify failure**

  Run:

  ```text
  bun test tests/provider-flow.test.ts tests/endpoint-flow.test.ts tests/model-catalog.test.ts tests/provider-operation-result.test.ts
  ```

  Expected: FAIL because current types and components still require `provider.models` and save `models`.

- [ ] **Step 3: Remove provider model fields from client types and commands**

  在 `relay.ts` 删除 `ProviderModel` 和 `Provider.models`。在 Tauri `ProviderSaveInput` 删除 `models`，创建和更新请求不再构造模型数组。保留 endpoint save 的 `models`，因为那是接入点事实。

- [ ] **Step 4: Derive provider models in the catalog helpers**

  将 `endpointModelsForProvider` 和相关可用模型函数改为根据 `Provider.provider_type` 调用 `modelCatalogProviderModels`，返回目录模型对象并在选项中使用 `id` 作为提交值、`display_name` 作为显示值。

  在 `useProviderForm` 中，已有供应商和新供应商都按当前 `provider_type` 推导模型展示；供应商保存 payload 只保留连接配置和能力覆盖。

- [ ] **Step 5: Update provider and endpoint components**

  - `ProviderList` 接收目录或调用目录索引计算模型数量。
  - `ProviderForm` 保留模型分组展示，但数据来自目录。
  - `EndpointForm` 的供应商筛选、模型选项和校验改为目录查询。
  - 不修改当前未提交的 Codex 推理设置文件和测试。

- [ ] **Step 6: Run focused Bun tests and verify pass**

  Run:

  ```text
  bun test tests/provider-flow.test.ts tests/endpoint-flow.test.ts tests/model-catalog.test.ts tests/provider-operation-result.test.ts
  bun run typecheck
  ```

  Expected: focused tests and typecheck PASS.

### Task 5: 跨仓引用清理和完整验证

**Files:**
- Modify only as required by compile/test failures in `prelay-protocol`, `prelay-server`, and `prelay-client`.
- Test: all affected Rust and Bun test suites.

**Interfaces:**
- Consumes: Tasks 1-4 的协议、服务端迁移和客户端目录推导。
- Produces: 无供应商模型副本的生产读写引用（迁移中的旧表删除引用除外）、无供应商请求模型副本、接入点主链路完整通过。

- [ ] **Step 1: Run recursive structure scans**

  Run:

  ```text
  rg -n --hidden -S "identity_provider_models|ProviderModelResponse|Provider\\.models|models: Some\\(input\\.models\\)" prelay-protocol prelay-server prelay-client -g '!target' -g '!node_modules' -g '!dist'
  ```

  Expected: no production read/write references; the PostgreSQL migration may retain one explicit `DROP TABLE identity_provider_models` reference, and historical documentation references must be reviewed separately.

- [ ] **Step 2: Run server formatting, tests, and clippy**

  Run:

  ```text
  cargo fmt --all --manifest-path prelay-server/Cargo.toml
  cargo test --manifest-path prelay-server/Cargo.toml
  cargo clippy --manifest-path prelay-server/Cargo.toml --all-targets --all-features -- -D warnings
  ```

  Expected: format, tests, and clippy pass. Any existing unrelated clippy failure must be reported with exact files and not attributed to this change.

- [ ] **Step 3: Run protocol formatting and tests**

  Run:

  ```text
  cargo fmt --all --manifest-path prelay-protocol/Cargo.toml
  cargo test --manifest-path prelay-protocol/Cargo.toml
  ```

  Expected: PASS.

- [ ] **Step 4: Run client tests, typecheck, and build**

  Run from `prelay-client`:

  ```text
  bun test
  bun run typecheck
  bun run build
  ```

  Expected: PASS while preserving the five pre-existing user-modified files.

- [ ] **Step 5: Review all three repository diffs**

  Run:

  ```text
  git -C prelay-protocol diff --check
  git -C prelay-server diff --check
  git -C prelay-client diff --check
  git -C prelay-protocol status --short
  git -C prelay-server status --short
  git -C prelay-client status --short
  ```

  Confirm the implementation changes, protocol submodule pointer, migration, tests, and client edits are within scope. Do not stage or commit without explicit authorization.

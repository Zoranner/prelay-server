# Extensions Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将扩展发现和 Rule/Skill 安装内容读取整合到 Prelay 服务端管理 API。

**Architecture:** 协议仓先定义管理 DTO 与错误码；服务端以 `extensions` 领域模块访问唯一 Gitea 上游并提供受认证路由；客户端只经 Tauri 调用管理 API，再写入本机智能体配置。目录查询按分类独立发起，不提供聚合接口。

**Tech Stack:** Rust、Axum、Reqwest、Tokio、Serde、Tauri 2、Nuxt 4、Bun。

**Spec:** `docs/superpowers/specs/2026-08-28-extensions-integration-design.md`

## Global Constraints

- 扩展接口固定在 `/api/extensions/*`，使用设备凭据认证，不进入 `/v1`。
- 目录必须按 Rule、Skill、Plugin、MCP 分开查询，不增加聚合接口或客户端并行加载。
- 只有 Rule 和 Skill 可安装；服务端只返回固定版本的受限文件包。
- 客户端不得直接访问 Gitea、保存 Gitea 令牌或配置扩展中心地址。
- Rust 修改后执行 `cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings` 与相称测试；Node.js 命令使用 Bun。

---

### Task 1: 扩展管理协议

**Files:**
- Create: `prelay-protocol/src/extensions.rs`
- Modify: `prelay-protocol/src/lib.rs`
- Modify: `prelay-protocol/src/error.rs`
- Test: `prelay-protocol/src/extensions.rs`

**Interfaces:**
- Produces: `ExtensionKind`、`ExtensionVersion`、`ExtensionSummary`、`ExtensionInstallBundle` 和扩展稳定错误码。

- [ ] **Step 1: 写出协议序列化测试**
- [ ] **Step 2: 运行 `cargo test`，确认扩展 DTO 尚不存在导致测试失败**
- [ ] **Step 3: 实现协议 DTO、导出与错误码**
- [ ] **Step 4: 运行 `cargo test`，确认协议序列化与错误码通过**
- [ ] **Step 5: 提交协议变更**

### Task 2: 服务端扩展领域与配置

**Files:**
- Create: `prelay-server/src/extensions/mod.rs`
- Create: `prelay-server/src/extensions/config.rs`
- Create: `prelay-server/src/extensions/package.rs`
- Create: `prelay-server/src/extensions/gitea.rs`
- Create: `prelay-server/src/extensions/catalog.rs`
- Modify: `prelay-server/src/lib.rs`
- Modify: `prelay-server/src/main.rs`
- Modify: `prelay-server/Cargo.toml`
- Test: `prelay-server/src/extensions/package.rs`
- Test: `prelay-server/src/extensions/catalog.rs`

**Interfaces:**
- Consumes: Task 1 的协议 DTO 和错误码。
- Produces: `ExtensionCatalog::from_environment`、分类目录、版本、README 与 Rule/Skill 安装包查询。

- [ ] **Step 1: 为 tag、commit、路径、类型判定和过期快照写失败测试**
- [ ] **Step 2: 运行对应 `cargo test`，确认新模块不存在导致测试失败**
- [ ] **Step 3: 实现配置校验、Gitea 客户端、目录快照和固定版本文件读取**
- [ ] **Step 4: 运行对应 `cargo test`，确认模块行为通过**
- [ ] **Step 5: 提交服务端领域变更**

### Task 3: `/api` 路由重命名与扩展路由

**Files:**
- Move: `prelay-server/src/routes/management/` to `prelay-server/src/routes/api/`
- Create: `prelay-server/src/routes/api/extensions.rs`
- Modify: `prelay-server/src/app.rs`
- Modify: `prelay-server/src/routes/mod.rs`
- Modify: `prelay-server/tests/protocol_routes.rs`
- Test: `prelay-server/tests/management_isolation.rs`

**Interfaces:**
- Consumes: Task 2 的 `AppState.extensions`。
- Produces: 设备凭据保护的 `/api/extensions/*` 路由。

- [ ] **Step 1: 为匿名拒绝、分类目录、版本、README 和不可安装类型写失败路由测试**
- [ ] **Step 2: 运行 `cargo test --test management_isolation`，确认新路由尚未注册导致测试失败**
- [ ] **Step 3: 改名路由目录并实现只做 HTTP 映射的扩展处理器**
- [ ] **Step 4: 运行路由与协议测试，确认认证边界和响应通过**
- [ ] **Step 5: 提交服务端路由变更**

### Task 4: 客户端本机安装链路

**Files:**
- Modify: `prelay-client/src-tauri/src/extensions.rs`
- Modify: `prelay-client/src-tauri/src/commands/extensions.rs`
- Test: `prelay-client/src-tauri/src/extensions.rs`

**Interfaces:**
- Consumes: Task 1 的协议 DTO，以及现有 `authenticated_api`。
- Produces: 按分类读取、读取 README、获取安装包并原子写入 Rule/Skill 目标目录的 Tauri 命令。

- [ ] **Step 1: 为服务端安装包写入规则和 Skill 的行为写失败测试**
- [ ] **Step 2: 运行 `cargo test extensions::tests --lib`，确认客户端仍依赖 Gitea 导致测试失败**
- [ ] **Step 3: 删除 Gitea 常量、Gitea DTO 与直接 HTTP 访问，改用受认证管理 API**
- [ ] **Step 4: 运行本机扩展测试，确认文件落点和拒绝 Plugin/MCP 的行为通过**
- [ ] **Step 5: 提交客户端原生命令变更**

### Task 5: 客户端分类状态与页面调用

**Files:**
- Modify: `prelay-client/app/stores/relay.ts`
- Modify: `prelay-client/app/composables/useExtensionCatalog.ts`
- Modify: `prelay-client/app/pages/agents.vue`
- Modify: `prelay-client/app/components/extensions/ExtensionCatalogTable.vue`
- Modify: `prelay-client/app/components/extensions/ExtensionDetailDrawer.vue`
- Modify: `prelay-client/app/components/extensions/ExtensionInstallModal.vue`
- Test: `prelay-client/tests/extensions-flow.test.ts`

**Interfaces:**
- Consumes: Task 4 的 `extensions_list(kind)`、`extension_readme(name, tag)` 与 `extensions_install(name, tag, clients)` 命令。
- Produces: 单分类延迟加载、详情延迟读取和本机安装交互。

- [ ] **Step 1: 写出当前分类首次进入才加载、切换服务地址失效和详情按版本读取的失败测试**
- [ ] **Step 2: 运行 `bun test tests/extensions-flow.test.ts`，确认旧的全量目录模型导致测试失败**
- [ ] **Step 3: 用按类型的缓存替换全量 `ExtensionCatalogSnapshot`，并更新表格、详情和安装参数**
- [ ] **Step 4: 运行 Bun 测试和 `bun run typecheck`，确认页面与类型通过**
- [ ] **Step 5: 提交客户端页面变更**

### Task 6: 部署配置与独立服务退役准备

**Files:**
- Modify: `prelay-server/deploy/.env.example`
- Modify: `prelay-server/README.md`
- Modify: `prelay-extensions/README.md`
- Modify: `prelay-extensions/deploy/.env.example`

**Interfaces:**
- Consumes: Task 2 的环境变量。
- Produces: 服务端部署配置与独立服务退役前的迁移说明。

- [ ] **Step 1: 检查环境示例与 README 是否仍将独立服务描述为运行依赖**
- [ ] **Step 2: 更新服务端部署说明，并将独立服务标记为待退役而非立即删除**
- [ ] **Step 3: 运行 `git diff --check`，确认文档和示例配置无格式错误**
- [ ] **Step 4: 在目录、详情和安装的端到端验证完成后提交部署说明变更**

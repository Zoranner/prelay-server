# 服务端目录结构重构实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将服务端源码和集成测试重组为表达领域、协议和方向职责的目录，并让每个 Rust 文件不超过 450 行。

**Architecture:** 每个批次先让该批的目录与行数测试失败，再只迁移该批职责并恢复通过。目录表达领域和协议，文件只表达目录内的具体职责；`mod.rs` 只声明、导出与组织，不成为业务门面。

**Tech Stack:** Rust 2021、Axum、SeaORM、Serde、Tokio、Cargo integration tests。

**Spec:** `docs/superpowers/specs/2026-08-29-server-structure-design.md`

## Global Constraints

- 只修改 `prelay-server`；不得触及 `prelay-client`、`prelay-protocol`、公开 DTO、稳定错误码、`/api/*` 或 `/v1/*` 路径。
- 所有 `src/` 与 `tests/` 下的 Rust 文件物理行数最终不超过 450 行；测试文件同样适用。
- 不再以 `identity_`、`encode_`、`decode_`、`extensions_`、`schema_` 文件名前缀表达可由目录表达的下级模块。
- 目录重组不扩大可见性，不增加迁移工具、兼容入口、平行抽象或运行配置。
- 每一批先新增仅覆盖该批目标的失败结构测试，再完成迁移并确认该测试通过；最终执行 `cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings` 与 `cargo test --all-targets --all-features`。
- 不终止用户进程，不使用 `CARGO_TARGET_DIR`；Cargo 被锁或超时只如实记录。

---

### Task 1: 桥接协议与流方向目录

**Files:**
- Create: `tests/source_layout.rs`
- Create: `src/bridge/anthropic/{mod.rs,decode.rs,encode.rs}`
- Create: `src/bridge/responses/{mod.rs,decode.rs,encode.rs}`
- Create: `src/bridge/stream/decode/{mod.rs,anthropic.rs,chat.rs,responses.rs}`
- Create: `src/bridge/stream/encode/{mod.rs,anthropic.rs,responses.rs}`
- Modify: `src/bridge/mod.rs`, `src/bridge/stream/mod.rs`
- Move: `src/bridge/anthropic_decode.rs`, `anthropic_encode.rs`, `responses_decode.rs`, `responses_encode.rs` 到对应协议目录。
- Move: `src/bridge/stream/decode_*.rs`、`encode_*.rs` 到对应方向目录。

**Interfaces:**
- Consumes: `InternalRequest`、`InternalResponse`、`BridgeDiagnostic` 与既有 SSE 转换函数。
- Produces: `bridge::anthropic::{decode,encode}`、`bridge::responses::{decode,encode}`、`bridge::stream::{decode,encode}` 内部模块路径。

- [ ] **Step 1: 写失败测试**

在 `tests/source_layout.rs` 新增 `bridge_modules_use_directories_and_stay_within_limit`：旧前缀文件不存在，新目录存在，`src/bridge/` 内每个 Rust 文件至多 450 行。

- [ ] **Step 2: 运行失败测试**

运行 `cargo test --test source_layout bridge_modules_use_directories_and_stay_within_limit`；预期因旧前缀文件和超长桥接文件失败。

- [ ] **Step 3: 迁移桥接代码**

协议目录中的 `decode.rs`、`encode.rs` 接管非流式转换；流式协议转换进入 `stream/decode/`、`stream/encode/`。`events.rs`、`pipeline.rs`、`sse.rs` 保持为流公共组件。按行为将内联测试拆入同级测试模块，更新所有 `use crate::bridge::*` 调用，且不改变函数签名。

- [ ] **Step 4: 验证并提交**

运行结构测试和覆盖 Anthropic、Responses、流转换的现有单元测试；随后运行 `cargo fmt --all`、`git diff --check`，以“重组桥接协议模块”提交。

### Task 2: Provider 协议实现目录

**Files:**
- Create: `src/providers/chat_completions/{mod.rs,request.rs,response.rs,stream.rs}`
- Create: `src/providers/spec/{mod.rs,capabilities.rs,urls.rs}`
- Modify: `src/providers/mod.rs`, `tests/source_layout.rs`
- Move: `src/providers/chat_completions.rs`、`src/providers/spec.rs` 到对应目录。

**Interfaces:**
- Consumes: `InternalRequest`、`InternalResponse`、`ProviderConfig`、`ProviderCapabilityOverrides`。
- Produces: 原有 Chat Completions 编码、解码、SSE 文本提取，以及 `UpstreamProtocol`、`ProviderSpec`、能力和 URL 解析函数的等价导出。

- [ ] **Step 1: 写失败测试**

添加 `provider_protocol_modules_use_directories_and_stay_within_limit`，断言两个旧平级文件不存在，新的请求、响应、流、能力、URL 文件存在且不超过 450 行。

- [ ] **Step 2: 运行失败测试**

运行 `cargo test --test source_layout provider_protocol_modules_use_directories_and_stay_within_limit`；预期因旧文件存在并超长失败。

- [ ] **Step 3: 迁移 Provider 实现**

将 Chat 请求编解码、响应解码和 SSE 提取分别放入 `request.rs`、`response.rs`、`stream.rs`。将 `ProviderSpec`、`UpstreamProtocol` 和能力判断放入 `capabilities.rs`，上游 URL 解析放入 `urls.rs`；通过两个 `mod.rs` 重新导出既有调用所需函数。

- [ ] **Step 4: 验证并提交**

运行结构测试与 `cargo test --lib providers::`；随后运行 `cargo fmt --all`、`git diff --check`，以“拆分供应商协议实现”提交。

### Task 3: Chat 与图像协议路由

**Files:**
- Create: `src/routes/v1/chat/{mod.rs,handler.rs,candidate.rs}`
- Create: `src/routes/v1/images/{mod.rs,handler.rs,candidate.rs,request_log.rs}`
- Modify: `src/routes/v1/mod.rs`, `tests/source_layout.rs`
- Move: `src/routes/v1/chat.rs`、`src/routes/v1/images.rs` 到对应协议目录。

**Interfaces:**
- Consumes: `CurrentProtocolAccess`、`ResolvedEndpointProvider`、`Storage`、既有上游策略和请求日志 DTO。
- Produces: 不变的 `POST /v1/chat/completions` 与 `POST /v1/images/generations` 路由、候选切换和请求日志行为。

- [ ] **Step 1: 写失败测试**

添加 `chat_and_images_routes_use_protocol_directories_and_stay_within_limit`，断言旧 `chat.rs`、`images.rs` 不存在，协议目录存在且每个文件不超过 450 行。

- [ ] **Step 2: 运行失败测试**

运行 `cargo test --test source_layout chat_and_images_routes_use_protocol_directories_and_stay_within_limit`；预期因旧路由文件存在并超长失败。

- [ ] **Step 3: 迁移路由**

每个 `mod.rs` 只注册原路径。`handler.rs` 保留请求解析、候选循环、重试与成功 Provider 记忆；`candidate.rs` 保留单候选上游调用；图像的 `request_log.rs` 独占日志构造与尽力写入。不得提取通用协议 handler，不得改变模型解析、上游 URL、流式响应或错误语义。

- [ ] **Step 4: 验证并提交**

运行结构测试和覆盖 Chat、图像、候选切换、请求日志的现有测试；随后运行 `cargo fmt --all`、`git diff --check`，以“重组 Chat 与图像协议路由”提交。

### Task 4: Responses 与 Messages 协议路由

**Files:**
- Create: `src/routes/v1/responses/{mod.rs,handler.rs,candidate.rs,native.rs,anthropic.rs,chat.rs,sessions.rs}`
- Create: `src/routes/v1/messages/{mod.rs,handler.rs,candidate.rs,native.rs,responses.rs,chat.rs}`
- Modify: `src/routes/v1/mod.rs`, `tests/source_layout.rs`
- Move: `src/routes/v1/responses.rs`、`src/routes/v1/messages.rs` 到对应协议目录。

**Interfaces:**
- Consumes: Responses、Anthropic Messages、Chat Completions 桥接和 Provider 编码函数，`ResponseSessionInsert` 与请求日志 DTO。
- Produces: 不变的 `POST /v1/responses`、`POST /v1/messages`、流式响应、会话续接、候选切换与统计记录。

- [ ] **Step 1: 写失败测试**

添加 `responses_and_messages_routes_use_protocol_directories_and_stay_within_limit`，断言两个旧文件不存在，新目录中入口、候选、协议专属调用和会话模块存在且均不超过 450 行。

- [ ] **Step 2: 运行失败测试**

运行 `cargo test --test source_layout responses_and_messages_routes_use_protocol_directories_and_stay_within_limit`；预期因 `responses.rs` 为 1990 行、`messages.rs` 为 1799 行失败。

- [ ] **Step 3: 迁移 Responses 路由**

`handler.rs` 保留入口解码和候选循环，`candidate.rs` 负责候选分派，`native.rs`、`anthropic.rs`、`chat.rs` 分别负责目标上游协议和流式转换，`sessions.rs` 保留会话读取与工具调用计数。按行为拆出内联测试，不复制断言。

- [ ] **Step 4: 迁移 Messages 路由**

`handler.rs` 保留入口和候选循环，`candidate.rs` 负责协议分派，`native.rs`、`responses.rs`、`chat.rs` 保留各协议转换、流式转换与请求日志。按行为拆出内联测试，不复制断言。

- [ ] **Step 5: 验证并提交**

运行结构测试和覆盖 `/v1/responses`、`/v1/messages`、会话、流式、候选失败的现有测试；随后运行 `cargo fmt --all`、`git diff --check`，以“拆分 Responses 与 Messages 协议路由”提交。

### Task 5: 实体、Schema 与 Storage 领域目录

**Files:**
- Create: `src/entity/identity/mod.rs`
- Create: `src/schema/{mod.rs,indexes.rs}` 与 `src/schema/tables/{mod.rs,identity.rs,providers.rs,endpoints.rs,sessions.rs,request_logs.rs,model_aliases.rs}`
- Create: `src/storage/{access.rs,request_logs.rs}`
- Modify: `src/entity/mod.rs`, `src/storage/mod.rs`, `src/storage/stats.rs`, `tests/source_layout.rs`
- Move: `src/entity/identity_*.rs` 到 `src/entity/identity/`。
- Move: `src/schema.rs` 到 `src/schema/`。

**Interfaces:**
- Consumes: SeaORM 实体、SQLite/PostgreSQL 初始化、`Storage` 和既有统计 DTO。
- Produces: 路径表达身份作用域的实体模块；不变的 `schema::initialize`；不变的 `Storage` 方法、事务与加密边界。

- [ ] **Step 1: 写失败测试**

添加 `persistence_modules_use_domain_directories_and_stay_within_limit`，断言 `entity/identity_*.rs` 与根层 `schema.rs` 不存在，身份实体与 schema 表、索引目录存在，Storage、Schema、实体文件均不超过 450 行。

- [ ] **Step 2: 运行失败测试**

运行 `cargo test --test source_layout persistence_modules_use_domain_directories_and_stay_within_limit`；预期因旧前缀、根层 schema 和超长持久化文件失败。

- [ ] **Step 3: 迁移实体和 Schema**

按 Provider、接入点、模型、会话、请求日志与别名迁移身份实体。`schema/mod.rs` 保留初始化入口，表定义按表域拆分，索引放入 `indexes.rs`；表名、列名、索引名、创建顺序、方言分支和幂等语义不得变化。

- [ ] **Step 4: 拆分 Storage 实现**

协议访问和当前 Provider 记忆迁入 `access.rs`，请求日志插入和流更新迁入 `request_logs.rs`，统计查询按 overview、列表、时间线拆分私有聚合辅助。`storage/mod.rs` 仅保留连接、密钥、错误、共享 DTO 和模块声明。

- [ ] **Step 5: 验证并提交**

运行结构测试、`cargo test --test schema_contract`、`cargo test --test schema_initialization` 和现有身份存储测试；随后运行 `cargo fmt --all`、`git diff --check`，以“按领域重组持久化模块”提交。

### Task 6: 集成测试按领域组织与全树门禁

**Files:**
- Create: `tests/extensions.rs` 与 `tests/extensions/{catalog.rs,routes.rs}`
- Create: `tests/identity.rs` 与 `tests/identity/{cleanup.rs,storage.rs}`
- Create: `tests/management.rs` 与 `tests/management/{identity.rs,providers.rs,endpoints.rs,stats.rs,provider_operations.rs}`
- Create: `tests/schema.rs` 与 `tests/schema/{contract.rs,initialization.rs}`
- Create: `tests/v1.rs` 与 `tests/v1/{identity_scope.rs,routes.rs}`
- Modify: `tests/source_layout.rs`
- Move: 所有 `extensions_*.rs`、`identity_*.rs`、`schema_*.rs`、`management_isolation.rs`、`protocol_routes.rs` 到对应领域目录。

**Interfaces:**
- Consumes: `tests/support`、`tests/test_context`、当前管理 API、协议路由与数据库行为。
- Produces: 短的 Cargo integration test 根入口和按稳定资源域组织的测试模块，不丢失任何既有断言。

- [ ] **Step 1: 写失败测试**

添加 `integration_tests_use_domain_directories_and_stay_within_limit`，断言旧前缀测试文件与 `management_isolation.rs` 不存在，根入口和领域目录存在，所有测试文件不超过 450 行。

- [ ] **Step 2: 运行失败测试**

运行 `cargo test --test source_layout integration_tests_use_domain_directories_and_stay_within_limit`；预期因旧测试文件和超长测试文件失败。

- [ ] **Step 3: 重组集成测试**

每个根入口只 `mod` 对应领域文件。将管理隔离断言按身份、Provider、接入点、统计和 Provider 操作分发；其余前缀测试迁入领域目录。`tests/support/` 只保留跨领域注册、认证与 fixture 工具。

- [ ] **Step 4: 完成全树门禁**

将 `source_layout.rs` 收敛为递归全树检查：每个 `.rs` 文件不超过 450 行，已迁移区域不存在旧前缀路径。删除过渡性 allowlist 和重复批次断言。

- [ ] **Step 5: 运行全量验证并提交**

运行：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
```

预期：全量门禁通过，`source_layout` 证明全树没有超限 Rust 文件或旧前缀目录边界。暂存本任务文件并以“按领域重组服务端集成测试”提交。

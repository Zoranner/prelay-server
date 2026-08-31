# 活动与自动记忆实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 统一活动运行记录，异步保存脱敏活动正文，并从中提炼可追溯、幂等的团队共享记忆。

**Architecture:** `identity_activities` 保存运行和统计字段，`activity_contents` 保存短期正文，`memories` 与 `memory_sources` 保存长期记忆和来源。协议请求完成后尽力投递正文；数据库 worker 以状态、租约和幂等 upsert 提炼记忆，绝不阻塞或递归进入原始调用。

**Tech Stack:** Rust 2021、Axum、Tokio、SeaORM、SeaQuery、Serde、SQLite、PostgreSQL、`prelay-protocol`、Tauri 管理 API。

**Spec:** `docs/superpowers/specs/2026-08-31-activity-memory-design.md`

## Global Constraints

- 领域术语统一使用“活动”。不得新增会话、消息、人员账号、手动 remember、上传文档或独立文档库。
- 来源只保存 `identity_id`、脱敏 evidence、形成时间；不增加贡献者前缀字段，不保存或强制关联活动 ID。
- 正文只保存规范化、脱敏且有上限的文本。不得保存凭据、请求头、完整原始 JSON、长期二进制附件、图片或截图内容。
- 第一阶段不增加 `/v1` 路由、公开记忆 API、外部队列、图数据库或向量数据库。最小读取能力为内部 `Storage::search_memories`。
- 提炼复用产生该活动的既有 Provider 与上游模型，直接调用上游；不经 `/v1`、不使用 Endpoint Token、不记录为新活动。
- SQLite 与 PostgreSQL 均支持同库的 `identity_request_logs` 到 `identity_activities` 表重命名；活动正文和记忆表只在已有完整活动 schema 上增量创建。不实现跨数据库迁移或通用迁移框架。
- 活动和正文随身份清理删除；记忆和来源不随正文清理。`memory_sources.identity_id` 不设外键，避免身份清理删除长期来源。
- 仅成功活动且具有足够脱敏文本的正文进入待提炼状态。正文、提炼、清理和检索失败都不得改变已发送的协议响应。
- 管理 DTO 或 `/api` 路径先改 `prelay-protocol`，再更新两个父仓的 submodule 和调用方。三个仓库分别提交，不推送、打 tag 或发布。
- 不改 Compose、CI、`.refs`、客户端视觉结构或无关 Provider/Endpoint 行为。客户端仅同步活动 DTO、Tauri command、路径、类型和既有活动页测试。
- 每个 Rust 批次完成后执行 `cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features`、`git diff --check`；文件锁只如实记录。

---

### Task 1: 先提交活动管理契约并同步活动页

**Files:**
- Modify: `prelay-protocol/src/stats.rs`, `prelay-protocol/src/lib.rs`, `prelay-protocol/tests/management_dto.rs`
- Rename: `prelay-protocol/docs/protocol/统计/请求记录.bru` to `prelay-protocol/docs/protocol/统计/活动.bru`
- Modify: `prelay-server/crates/protocol`, `prelay-server/src/{error.rs,stats.rs,storage/request_logs.rs}`, `prelay-server/src/routes/api/stats.rs`, `prelay-server/tests/management/stats.rs`
- Modify: `prelay-client/crates/protocol`, `prelay-client/src-tauri/src/commands/stats.rs`, `prelay-client/app/stores/relay.ts`, `prelay-client/app/pages/stats.vue`, `prelay-client/app/components/activity/RequestTable.vue`, `prelay-client/tests/stats-flow.test.ts`

**Interfaces:** `ActivitySummary` 保持 `RequestLogSummary` 的 JSON 字段；`GET /api/stats/activities?limit=<usize>`；Tauri `stats_activities(limit)`；TS `Activity`。统计聚合字段暂不改名，避免量纲变更混入活动记录改名。

**Ruling:** `prelay-server` 固定的旧协议提交包含 MCP 安装 DTO，但当前协议权威分支缺失该契约；先在 `prelay-protocol` 恢复相同 DTO 与 JSON 测试，再更新两个父仓指针。服务端 `request_logs` 的内部文件、表和 Storage 方法统一留在 Task 2 原子迁移，本任务只将其公开返回 DTO 切换为 `ActivitySummary`。

- [ ] 写 `ActivitySummary` round-trip RED，断言旧 `RequestLogSummary` 不再导出。
- [ ] 在协议仓实现 DTO/根导出改名，Bruno 标题改为“获取活动”、URL 改为 `/api/stats/activities`；完整 Rust 验证后提交“统一活动管理契约”。
- [ ] 更新两个父仓 submodule。服务端改为 `ActivitySummary` 与 `/stats/activities`，内部 `list_request_logs` 留待 Task 2；客户端只同步 native command、页面调用、`Activity` 类型、表格空文案和“活动诊断”，保留既有视觉结构。
- [ ] 服务端运行 `cargo test --test management stats`；客户端运行 `bun test tests/stats-flow.test.ts`、`bun run typecheck` 和 `src-tauri` Rust 门禁；检查 diff 后分仓提交。

### Task 2: 把运行记录完整改名为活动并限定空库发布

**Files:**
- Create: `prelay-server/src/entity/identity/activities.rs`, `prelay-server/src/schema/tables/activities.rs`, `prelay-server/src/storage/activities.rs`
- Modify: `prelay-server/src/entity/{identities.rs,identity/mod.rs}`, `prelay-server/src/schema/{mod.rs,indexes.rs,tables/mod.rs}`, `prelay-server/src/storage/{mod.rs,stats.rs,identities.rs}`, `prelay-server/src/observability/stream_stats/**`, `prelay-server/src/routes/v1/**`, `prelay-server/tests/**`, `prelay-server/README.md`
- Remove: `prelay-server/src/entity/identity/request_logs.rs`, `prelay-server/src/schema/tables/request_logs.rs`, `prelay-server/src/storage/request_logs.rs`

**Interfaces:** `ActivityInsert`、`StreamActivityUpdate`、`Storage::insert_activity*`、`Storage::update_stream_activity`、`Storage::list_activities`；physical table `identity_activities`；index `idx_identity_activities_identity_created_at`。

- [ ] 先令 schema contract 和 source-layout 断言 `identity_activities`、新索引和新模块存在；当前代码应 RED，并覆盖完整旧活动表迁移后数据仍可读。
- [ ] 原子替换 `RequestLog*`、`insert_request_log*`、`update_stream_request_log`、`list_request_logs`、entity、schema、索引和身份清理名称；不改变元数据字段、价格、诊断、失败转移或流式行为。
- [ ] 更新 schema 初始化测试：完整旧 schema 使用同库 `ALTER TABLE` 迁移并重建活动索引，真正缺表的 schema 仍拒绝启动；README 说明不支持跨数据库迁移。
- [ ] 运行 schema、management stats、全部 v1、stream stats 测试和 Rust 门禁，提交“统一服务端活动模型”。

### Task 3: 建立活动正文和记忆的跨数据库核心表

**Files:**
- Create: `prelay-server/src/entity/{activity_contents.rs,memories.rs,memory_sources.rs}`, `prelay-server/src/schema/tables/{activity_contents.rs,memories.rs,memory_sources.rs}`
- Modify: `prelay-server/src/entity/mod.rs`, `prelay-server/src/schema/{mod.rs,indexes.rs,tables/mod.rs}`, `prelay-server/src/storage/mod.rs`, `prelay-server/tests/schema/{contract.rs,initialization.rs}`

**Interfaces:** 一活动最多一正文；`memories.normalized_key` 唯一；来源按 `(memory_id, identity_id, evidence_hash, observed_at)` 唯一、无 activity ID 和 identity FK。

- [ ] 新增 RED contract：`activity_contents` 包含内容、截断、hash、状态、次数、下次时间、lease、错误和完成时间；`memories` 包含 key、冲突 key、类型、状态、内容、置信度；`memory_sources` 包含 memory、identity、evidence、hash、时间。
- [ ] `activity_contents.activity_id` 外键指向 `identity_activities` 并唯一；来源仅对 memory 设外键；新增领取与检索的公共索引。
- [ ] 用 `String` 保存 UUID、时间和状态，用 `big_integer` 保存次数、`boolean` 保存截断、`double` 保存置信度；加入空库初始化与实体导出。
- [ ] 运行 schema 及 Mock PostgreSQL 初始化测试、Rust 门禁后提交“建立活动正文与记忆存储结构”。

### Task 4: 实现正文规范化、脱敏和非流式采集

**Files:**
- Create: `prelay-server/src/activity/{mod.rs,content.rs,redaction.rs,normalization.rs,tests.rs}`, `prelay-server/src/storage/activity_contents.rs`
- Modify: `prelay-server/src/{lib.rs,storage/mod.rs}`, `prelay-server/src/routes/v1/{chat,images,messages,responses}/**`, `prelay-server/tests/v1/**`

**Interfaces:** `ActivityContentDraft { activity_id, input_text, output_text, media_metadata_json, is_truncated, content_hash }` 和 `Storage::enqueue_activity_content(draft)`，初始状态 `pending`。

- [ ] 为 `normalize_activity_content` 写 RED：只收文本，删除 authorization/API key/bearer/设备凭据/endpoint token，按 UTF-8 字节上限截断，相同输入生成同 hash；图片仅保留类型、大小与已有文本，不含 base64。
- [ ] 在 Responses/Messages 从 `InternalRequest`/`InternalResponse` 采集文本，在 Chat 从 messages/choice 采集，在图像仅保留脱敏 prompt 和媒体元数据；不得序列化完整 JSON，失败活动不排队。
- [ ] 仅在活动元数据写入成功后尽力入队；失败只写不含正文、密钥、SQL 的 warning。为每种非流式协议断言“写入正文”与“正文失败不影响成功响应”。
- [ ] 运行 activity 单元、v1、schema 与 Rust 门禁后提交“采集脱敏活动正文”。

### Task 5: 为流式活动增加文本旁路采集

**Files:**
- Create: `prelay-server/src/activity/stream.rs`
- Modify: `prelay-server/src/activity/mod.rs`, `prelay-server/src/observability/stream_stats/{record.rs,state.rs}`, `prelay-server/src/routes/v1/{chat,images,messages,responses}/**`, `prelay-server/src/bridge/stream/**`, `prelay-server/src/observability/stream_stats/tests.rs`, `prelay-server/tests/v1/**`

**Interfaces:** `ActivityContentCapture::observe_text(&str)` / `finish()`；成功完成的流至多写一正文，不改变传输 bytes、顺序、首 token 或 usage。

- [ ] 为 Chat、Messages、Responses、图像流添加 RED：SSE 输出逐字节等价；文本完成后正文仅含解码文本；空流、错误、取消不进入 pending；图像不保存 bytes。
- [ ] 在 `record_stream_with_log_id` 的首块活动写入成功后创建有上限 capture，针对已解码文本更新；只在 completed 时尽力入队，绝不缓存 SSE、header 或二进制 chunk。
- [ ] 运行全部流测试与 stream stats 门禁，确认首 token、最终 usage、候选切换、错误状态未变，提交“采集流式活动正文”。

### Task 6: 实现记忆存储、精确去重和最小内部检索

**Files:**
- Create: `prelay-server/src/memory/{mod.rs,model.rs,normalization.rs,storage.rs,search.rs,tests.rs}`, `prelay-server/src/storage/memories.rs`
- Modify: `prelay-server/src/{lib.rs,storage/mod.rs}`, `prelay-server/src/entity/mod.rs`, `prelay-server/tests/identity/cleanup.rs`

**Interfaces:** `MemoryCandidate { kind, content, confidence, evidence, observed_at }`；`Storage::upsert_memory(candidate, identity_id)`；`Storage::search_memories(MemorySearch)`。

- [ ] 写 RED：同 key 重复处理只建一记忆；同来源唯一键只建一来源；多身份来源均保留；同 conflict key 的不同事实均保存并标记 `conflicted`；低置信候选保持 `pending_review`。
- [ ] `normalized_key` 由 kind/content 生成，`conflict_key` 由实体/属性生成；事务内读取或创建记忆再插入来源，不覆盖来源。`MemorySearch` 仅支持 query、kind、status、identity_id、时间和 `LIKE` 排序。
- [ ] 身份清理先删活动/正文，绝不删记忆/来源；测试身份删除后来源仍含原 identity ID 与 evidence。运行 memory、identity cleanup、schema 门禁后提交“保存可追溯团队记忆”。

### Task 7: 增加 worker、提炼适配器、租约和重试

**Files:**
- Create: `prelay-server/src/memory/{extractor.rs,worker.rs}`, `prelay-server/src/storage/memory_work.rs`
- Modify: `prelay-server/src/{main.rs,lib.rs,storage/mod.rs,memory/mod.rs}`, `prelay-server/tests/support/mod.rs`, `prelay-server/tests/v1/**`

**Interfaces:** `Storage::claim_due_activity_contents(worker_id, now, lease_until, limit)`；`MemoryWorker::run(storage, client, config)`；`MemoryExtractor::extract(&ClaimedActivityContent) -> Vec<MemoryCandidate>`；状态 `pending|retryable -> processing -> completed|retryable|failed`。

- [ ] 写 RED：双 worker 仅一方领取、过期 lease 可重领、成功 completed、重试有次数/安全错误/next attempt、超过上限才 failed、重复处理不重复记忆来源、worker 失败不影响 `/v1` 响应。
- [ ] 在事务中找 due/过期 ID，再条件更新 `status`、owner、expiry；只接收 `rows_affected == 1`。完成、重试、终止更新均加 `id + lease_owner`，防止过期 worker 覆盖新领取者。
- [ ] 按 activity 的 `identity_id + provider_id + model_upstream` 加载既有 Provider，通过现有 client/timeout 提炼。输入仅为脱敏正文和固定 JSON 候选说明，输出再验证 kind/content/confidence/evidence；不保存原文、不调 `/v1`、不产生活动、不加不支持 Provider 的兼容路径。
- [ ] `MemoryRuntimeConfig::from_environment()` 支持 `MEMORY_ENABLED`（默认 false）及正文上限、保留天数、轮询、lease、最大次数五个正整数配置。仅 enabled 时启动一个 worker；配置非法阻止启动，单次失败只安全告警。验证后提交“异步提炼团队记忆”。

### Task 8: 独立清理正文并完成收口

**Files:**
- Create: `prelay-server/src/activity/cleanup.rs`
- Modify: `prelay-server/src/{activity/mod.rs,storage/activity_contents.rs,main.rs}`, `prelay-server/README.md`, `prelay-server/tests/identity/cleanup.rs`, `prelay-server/tests/schema/contract.rs`

**Interfaces:** `Storage::delete_expired_activity_contents(now, retention)`；启动时和每日正文清理；正文清理不删除活动、记忆或来源。

- [ ] 写 RED：过期正文删除、未过期正文保留、已由过期正文生成的记忆和来源保留；保留天数为零或非数字使配置解析失败。
- [ ] 按 identity cleanup 的启动/每日模式调度正文清理；错误只记录 error code 与稳定 failure kind，不记录正文、evidence、数据库错误详情或凭据。
- [ ] README 记录空库前提、`MEMORY_*` 配置、正文脱敏/保留、非审计边界和失败不影响协议响应，不改 Compose/CI。
- [ ] 服务端执行完整 Rust 门禁与 `git diff --check`；分别检查协议仓、客户端的 submodule、工作树和 diff。仅已完成相称验证的逻辑单元分仓中文提交；不推送、打 tag、发布或改变远端状态。

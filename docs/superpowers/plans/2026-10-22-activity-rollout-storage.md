# 活动 Rollout 文件存储实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用每活动一份 JSONL rollout 文件替代 `activity_contents`，把完整对话事件保存下来，数据库只保留索引和运行元数据。

**Architecture:** 文件是唯一正文源，按 `sessions/YYYY/MM/DD/rollout-<时间>-<activity_id>.jsonl` 写入；PostgreSQL 的 `activity_rollouts` 只保存路径、状态、大小、hash 和 session 关联。写入期间使用 `.part`，完成后原子重命名；不引入归档、数据库正文副本或第二套记忆队列。

**Tech Stack:** Rust 2021、Axum、Tokio、SeaORM、PostgreSQL、serde_json、sha2、chrono、tempfile（仅测试）。

**Spec:** `docs/superpowers/specs/2026-10-22-activity-rollout-storage-design.md`

## Global Constraints

- 文件是唯一正文源，`activity_rollouts` 不得保存 `content`、`item_json` 或完整事件内容。
- 每个 activity 一个文件，不按 session 追加，不建立 `archived_sessions`。
- 不设置正文大小上限；`PROTOCOL_MAX_REQUEST_BODY_BYTES` 保持默认 256 MiB，除非出现真实 `413` 证据。
- 所有写入内容先经过现有凭据脱敏；不得保存 Authorization、Cookie、API Key、Endpoint Token 或设备凭据。
- 不把图片、音频、附件或 base64 内容直接写入 JSONL；只保存元数据和文件引用。
- 不新增记忆提取 worker；`memories` 和 `memory_sources` 保持现有结构，后续直接读取 rollout 文件。
- 每个 Rust 源文件不得超过 450 行；需要拆分时按职责拆分，不按技术层机械拆分。
- 不修改工作区中已有的 2026-10-19 未跟踪文档。
- 工作区已有的 `src/image_budget.rs` 修改不属于本计划，不得覆盖、回退或混入本计划提交。
- 每个任务完成后只做本地验证和 review checkpoint；提交、推送和打标签仍分别需要用户授权。

## 目标文件结构

```text
src/
  activity/
    mod.rs
    events.rs
    redaction.rs
    rollout.rs
    rollout_tests.rs
    stream.rs
    tests.rs
  storage/
    mod.rs
    recordings.rs
    rollouts.rs
  entity/
    activity_rollouts.rs
  schema/
    mod.rs
    activity_rollout.rs
    tables/activity_rollouts.rs
  routes/v1/
    chat/candidate.rs
    responses/{chat,native}.rs
    images/{activity,candidate}.rs
tests/
  activity_rollouts.rs
  schema/{contract,initialization}.rs
```

`activity/config.rs`、`activity/content.rs`、`storage/activity_contents.rs`、`entity/activity_contents.rs`、`schema/tables/activity_contents.rs` 和 `schema/activity_content.rs` 最终删除。

## Rollout 事件模型与写入器

**Task ID:** 1

**Files:**
- Create: `src/activity/events.rs`
- Create: `src/activity/rollout.rs`
- Create: `src/activity/rollout_tests.rs`
- Modify: `src/activity/mod.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `chrono::Utc`, `serde_json::Value`, `sha2::Sha256`
- Produces:

```rust
pub const DEFAULT_ACTIVITY_ROLLOUT_DIR: &str = "data/sessions";

pub struct RolloutStore { root: PathBuf }

impl RolloutStore {
    pub fn new(root: PathBuf) -> Self;
    pub fn from_environment() -> Result<Self, String>;
    pub fn begin(
        &self,
        meta: SessionMetaEvent,
        request_events: Vec<RolloutEventBody>,
    ) -> Result<RolloutWriter, RolloutError>;
    pub fn absolute_path(&self, relative: &str) -> PathBuf;
    pub fn read_events(&self, relative: &str) -> Result<Vec<RolloutEvent>, RolloutError>;
}

pub struct RolloutWriter;

impl RolloutWriter {
    pub fn append(&mut self, event: RolloutEventBody) -> Result<(), RolloutError>;
    pub fn summary(&self) -> RolloutSummary;
    pub fn finish(&mut self, status: RolloutStatus) -> Result<RolloutSummary, RolloutError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RolloutStatus {
    Writing,
    Completed,
    Failed,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutSummary {
    pub activity_id: String,
    pub session_id: String,
    pub relative_path: String,
    pub status: RolloutStatus,
    pub event_count: u64,
    pub byte_size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RolloutEvent {
    pub seq: u64,
    pub timestamp: String,
    #[serde(flatten)]
    pub body: RolloutEventBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum RolloutEventBody {
    SessionMeta(SessionMetaEvent),
    Message(MessageEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Reasoning(ReasoningEvent),
    Usage(UsageEvent),
    Error(ErrorEvent),
    SessionEnd(SessionEndEvent),
}

pub struct SessionMetaEvent {
    pub activity_id: String,
    pub session_id: String,
    pub identity_id: String,
    pub protocol_in: String,
    pub protocol_out: String,
    pub protocol_upstream: Option<String>,
    pub endpoint_name: String,
    pub model_requested: String,
    pub model_upstream: String,
    pub started_at: String,
}

pub struct MessageEvent {
    pub phase: EventPhase,
    pub role: String,
    pub content: serde_json::Value,
}

pub struct ToolCallEvent {
    pub phase: EventPhase,
    pub tool_call_id: String,
    pub name: String,
    pub arguments: String,
}

pub struct ToolResultEvent {
    pub phase: EventPhase,
    pub tool_call_id: String,
    pub content: serde_json::Value,
}

pub struct ReasoningEvent {
    pub phase: EventPhase,
    pub content: String,
}

pub struct UsageEvent {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
}

pub struct ErrorEvent {
    pub code: Option<String>,
    pub message: String,
    pub http_status: Option<u16>,
}

pub struct SessionEndEvent {
    pub status: RolloutStatus,
    pub http_status: Option<u16>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

pub enum EventPhase {
    Request,
    Response,
}

#[derive(Debug)]
pub enum RolloutError {
    Io(std::io::Error),
    Serialize(serde_json::Error),
    PartialLine { path: PathBuf, valid_events: usize },
}
```

给 `RolloutEvent`、`RolloutEventBody`、全部 payload 结构和 `EventPhase` 派生
`Debug, Clone, Serialize, Deserialize, PartialEq`；`EventPhase` 使用
`#[serde(rename_all = "snake_case")]`。

- [ ] **Step 1: Write failing writer tests**

Add `tempfile` under `[dev-dependencies]`, then test:

```rust
#[test]
fn writes_events_in_order_and_renames_part_file() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let mut writer = store
        .begin(test_meta("activity-1"), vec![test_request_message()])
        .expect("begin rollout");
    writer.append(test_response_message()).expect("append response");
    let summary = writer
        .finish(RolloutStatus::Completed)
        .expect("finish rollout");

    assert!(summary.relative_path.ends_with(".jsonl"));
    assert!(!root.path().join(&summary.relative_path).with_extension("jsonl.part").exists());
    let events = store.read_events(&summary.relative_path).expect("read events");
    assert_eq!(events.len(), 3);
    assert!(matches!(events[0].body, RolloutEventBody::SessionMeta(_)));
}

#[test]
fn preserves_content_larger_than_the_old_64k_limit() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let large = "x".repeat(1024 * 1024);
    let mut writer = store
        .begin(
            test_meta("activity-large"),
            vec![RolloutEventBody::Message(MessageEvent {
                phase: EventPhase::Request,
                role: "user".to_string(),
                content: serde_json::Value::String(large.clone()),
            })],
        )
        .expect("begin large rollout");
    let summary = writer
        .finish(RolloutStatus::Completed)
        .expect("finish large rollout");
    let events = store.read_events(&summary.relative_path).expect("read events");
    let RolloutEventBody::Message(message) = &events[1].body else {
        panic!("expected message event");
    };
    assert_eq!(message.content, serde_json::Value::String(large));
}
```

- [ ] **Step 2: Run the failing tests**

Run: `cargo test --lib activity::rollout`

Expected: fail because `RolloutStore`, `RolloutWriter` and event types do not exist.

- [ ] **Step 3: Implement the writer**

Use the exact path format `%Y/%m/%d` plus `rollout-%Y-%m-%dT%H-%M-%S-<activity_id>.jsonl`. Write to `.part`; serialize one event per line; update `Sha256` with the serialized line; `flush` after each event; `sync_all` before rename. `finish` appends a `SessionEnd` event, renames to `.jsonl`, and returns the summary. `RolloutStore::read_events` must reject a file whose last line is incomplete and return `RolloutError::PartialLine` with the valid prefix.

- [ ] **Step 4: Add redaction tests**

Test that `Authorization: Bearer secret`, `api_key=secret`, `sk-live-secret` and a private key block never appear in a written file, while non-secret text is unchanged.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib activity::rollout`

Expected: all pass.

**Review checkpoint:** stop after writer tests pass; do not wire it into routes yet.

## Activity Rollout Schema

**Task ID:** 2

**Files:**
- Create: `src/entity/activity_rollouts.rs`
- Create: `src/schema/tables/activity_rollouts.rs`
- Create: `src/schema/activity_rollout.rs`
- Modify: `src/entity/mod.rs`
- Modify: `src/schema/tables/mod.rs`
- Modify: `src/schema/indexes.rs`
- Modify: `src/schema/mod.rs`
- Modify: `tests/schema/contract.rs`
- Modify: `tests/schema/initialization.rs`

**Interfaces:**
- Consumes: `identity_activities(id)` and the existing migration table.
- Produces:

```rust
pub struct InsertActivityRollout {
    pub activity_id: String,
    pub identity_id: String,
    pub session_id: String,
    pub rollout_path: String,
    pub status: String,
    pub created_at: String,
}
```

- [ ] **Step 1: Write the failing schema test**

Replace `activity_contents` in the expected table list with `activity_rollouts` and assert:

- `activity_rollouts.activity_id` is `TEXT`/`VARCHAR`.
- `event_count` and `byte_size` are `BIGINT`.
- `rollout_path` has a unique index.
- `activity_contents` does not exist after initialization.

- [ ] **Step 2: Run the failing test**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test schema contract`

Expected: fail because the table is not created and `activity_contents` is still present.

- [ ] **Step 3: Add the table and migration**

Create `activity_rollouts` with the columns in the spec. In `schema::initialize`, run
`activity_rollout::apply_in(&transaction)` before `validate_current_schema`; do not run
the migration after the transaction commits. It must:

```sql
CREATE TABLE IF NOT EXISTS activity_rollouts (...);
DROP TABLE IF EXISTS activity_contents;
CREATE UNIQUE INDEX IF NOT EXISTS uq_activity_rollouts_path ON activity_rollouts (rollout_path);
CREATE INDEX IF NOT EXISTS idx_activity_rollouts_identity_created_at ON activity_rollouts (identity_id, created_at);
CREATE INDEX IF NOT EXISTS idx_activity_rollouts_session ON activity_rollouts (session_id);
```

Record migration version `activity_rollout_v1` in `prelay_schema_migrations`.

- [ ] **Step 4: Remove old schema expectations**

Remove `activity_contents` from `MEMORY_TABLES`, `initialize_memory_schema`, `tests/schema/contract.rs`, and the stale-content migration test. Replace the latter with a test that seeds `activity_contents`, calls `initialize`, and asserts the table is gone while `activity_rollouts` exists.

- [ ] **Step 5: Run schema tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test schema`

Expected: all pass.

## Activity Event Builders

**Task ID:** 3

**Files:**
- Modify: `src/activity/events.rs`
- Modify: `src/activity/mod.rs`
- Create: `src/activity/event_tests.rs`

**Interfaces:**
- Consumes: `InternalRequest`, `InternalResponse`, Chat payloads, `RawStreamContentCapture`, `StreamStatsSnapshot`.
- Produces:

```rust
pub fn events_from_internal_request(request: &InternalRequest) -> Vec<RolloutEventBody>;
pub fn events_from_internal_response(response: &InternalResponse) -> Vec<RolloutEventBody>;
pub fn events_from_chat_request(payload: &Value) -> Vec<RolloutEventBody>;
pub fn events_from_chat_response(payload: &Value) -> Vec<RolloutEventBody>;
pub fn events_from_stream_capture(capture: &RawStreamContentCapture) -> Vec<RolloutEventBody>;
pub fn events_from_stream_snapshot(snapshot: &StreamStatsSnapshot) -> Vec<RolloutEventBody>;
```

- [ ] **Step 1: Write failing event tests**

Use existing fixtures to assert:

- `InternalRequest` messages preserve role and text.
- `InternalRequest.instructions` becomes a `system` message.
- Assistant `tool_calls` become `tool_call` events.
- Tool messages with `tool_call_id` become `tool_result` events.
- Chat `message.tool_calls` and `tool_call_id` are not dropped.
- `InternalResponse::FunctionToolCall` is not dropped.
- A 1 MiB message is preserved.

- [ ] **Step 2: Run the failing tests**

Run: `cargo test --lib activity::event_tests`

Expected: fail because builders do not exist.

- [ ] **Step 3: Implement builders**

Recursively redact all JSON string values before creating events. Keep `content` as `Value` so arrays and structured blocks survive. Do not call `activity_content_from_text` or any truncation helper. For stream capture, emit one assistant `message` for non-empty text followed by one `tool_call` per captured tool call; mark the event phase as `response`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib activity::event_tests`

Expected: all pass.

## AppState and Storage Index

**Task ID:** 4

**Files:**
- Create: `src/storage/rollouts.rs`
- Modify: `src/storage/mod.rs`
- Modify: `src/storage/recordings.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Modify: `tests/support/mod.rs`
- Modify: `src/test_support.rs`

**Interfaces:**
- Consumes: `RolloutSummary`, `RolloutStore`.
- Produces:

```rust
impl Storage {
    pub async fn insert_activity_with_rollout(
        &self,
        identity_id: &str,
        activity_id: String,
        activity: ActivityInsert,
        rollout: RolloutSummary,
    ) -> Result<(), StorageError>;

    pub async fn start_stream_activity_with_rollout(
        &self,
        identity_id: &str,
        activity_id: &str,
        activity: ActivityInsert,
        rollout: RolloutSummary,
    ) -> Result<(), StorageError>;

    pub async fn complete_activity_rollout(
        &self,
        activity_id: &str,
        rollout: RolloutSummary,
    ) -> Result<(), StorageError>;

    pub async fn find_activity_rollout(
        &self,
        activity_id: &str,
    ) -> Result<Option<activity_rollouts::Model>, StorageError>;
}
```

`AppState` gains:

```rust
pub rollout_store: crate::activity::rollout::RolloutStore,
```

The orchestration function used by routes is:

```rust
pub async fn record_activity_with_events(
    storage: &Storage,
    rollout_store: &RolloutStore,
    identity_id: &str,
    activity: ActivityInsert,
    session_id: Option<&str>,
    request_events: Vec<RolloutEventBody>,
    response_events: Vec<RolloutEventBody>,
) -> Result<(), StorageError>;
```

`session_id` defaults to the generated `activity_id` when the protocol has no conversation key.

- [ ] **Step 1: Write failing storage tests**

Test that inserting an activity with a rollout creates both `identity_activities` and `activity_rollouts`, and that the path is relative. Test that a duplicate activity ID fails and leaves the rollout row absent. Test that `complete_activity_rollout` updates status, byte size, event count and hash.

- [ ] **Step 2: Run failing tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --lib storage::rollouts`

Expected: fail because the methods and table module do not exist.

- [ ] **Step 3: Implement**

Insert the activity and rollout row in one transaction. Do not put file bytes in the database. `start_stream_activity_with_rollout` inserts status `writing`; `complete_activity_rollout` updates the row and sets `completed_at`. If the rollout path does not exist at completion, return `StorageError::ValidationFailed`.

- [ ] **Step 4: Initialize AppState**

In `main`, call `RolloutStore::from_environment()` before building `AppState` and store it. In `test_support`, create a temporary root with `tempfile::tempdir()` and keep the guard alive for the test scope.

- [ ] **Step 5: Run tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --lib storage::rollouts`

Expected: all pass.

## Non-Streaming Responses

**Task ID:** 5

**Files:**
- Modify: `src/routes/v1/responses/{native,chat}.rs`
- Modify: `src/activity/mod.rs`
- Modify: `src/storage/recordings.rs`
- Modify: `src/routes/v1/responses/tests/activities.rs`

**Interfaces:**
- Consumes: `events_from_internal_request`, `events_from_internal_response`.
- Produces: one rollout per successful non-streaming activity.

- [ ] **Step 1: Write the failing integration assertions**

After a successful non-streaming Responses request, assert:

- `activity_contents` is not present.
- `activity_rollouts` has one row for the activity.
- The JSONL contains `session_meta`, a request message, an assistant message and `session_end`.
- A tool-call response contains a `tool_call` event with the full arguments.

- [ ] **Step 2: Run the failing tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test v1 responses::tests::activities`

Expected: fail because the routes still use `insert_activity_with_content`.

- [ ] **Step 3: Replace the write path**

Replace `insert_activity_with_content` with:

```rust
record_activity_with_events(
    &state.storage,
    &state.rollout_store,
    &context.identity_id,
    activity,
    session_id.as_deref(),
    events_from_internal_request(request),
    events_from_internal_response(response),
)
```

Keep the existing HTTP response bytes, status codes and headers unchanged.

- [ ] **Step 4: Run tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test v1 responses::tests::activities`

Expected: all pass; no test reads `activity_contents`.

## Non-Streaming Chat and Images

**Task ID:** 6

**Files:**
- Modify: `src/routes/v1/chat/candidate.rs`
- Modify: `src/routes/v1/images/activity.rs`
- Modify: `src/routes/v1/images/candidate.rs`
- Modify: `src/routes/v1/images/tests/activities.rs`
- Modify: `src/routes/v1/chat/tests/activities.rs`

**Interfaces:**
- Consumes: `events_from_chat_request`, `events_from_chat_response`.
- Produces: Chat and image activities use the same rollout writer.

- [ ] **Step 1: Write failing tests**

For Chat, assert assistant `tool_calls` become `tool_call` events and tool results in later request messages become `tool_result` events. For images, assert the file contains `session_meta`, a request message, media metadata and `session_end`, and never contains base64 image bytes.

- [ ] **Step 2: Run failing tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test v1 chat::tests::activities`

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test v1 images::tests::activities`

Expected: fail because the routes still write `activity_contents`.

- [ ] **Step 3: Implement**

Replace `insert_activity_with_content` and `record_completed_activity` with the rollout path. For images, write only `media_type`, `size_bytes` and `sha256`; do not write response bytes. Preserve the existing best-effort behavior for successful image responses.

- [ ] **Step 4: Run tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test v1 chat::tests::activities`

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test v1 images::tests::activities`

Expected: all pass.

## Streaming Rollout

**Task ID:** 7

**Files:**
- Modify: `src/observability/stream_stats/{state,record,persistence}.rs`
- Modify: `src/bridge/stream/events.rs`
- Modify: `src/activity/stream.rs`
- Modify: `src/routes/v1/responses/{native,chat}.rs`
- Modify: `src/routes/v1/chat/candidate.rs`
- Modify: `src/routes/v1/images/candidate.rs`
- Modify: `src/observability/stream_stats/{tests,content_tests}.rs`

**Interfaces:**
- `StreamRecordOptions` gains `rollout: Option<RolloutWriter>` and `request_events: Vec<RolloutEventBody>`.
- `record_stream_with_rollout` and `record_first_chunk_with_rollout` replace the content-named variants.
- On normal EOF, failed upstream, empty stream and downstream disconnect, the writer must always append `session_end`.
- Add `RawStreamContentCapture::captured_tool_calls()` and `StreamStatsSnapshot::captured_tool_calls()` so `activity/events.rs` can emit structured tool-call events without exposing mutable internals.

- [ ] **Step 1: Write failing stream tests**

Cover:

- Normal stream writes `session_meta`, request events, response message, usage and `session_end`.
- Tool-call stream writes full arguments once.
- Upstream error keeps partial text and writes `error` + `session_end` with `failed`.
- Client drops the stream and the file is finalized with `partial` or `failed`, not left `writing`.
- A stream body may be larger than 64 KiB without truncation.

- [ ] **Step 2: Run failing tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --lib observability::stream_stats`

Expected: fail because streaming still writes `activity_contents`.

- [ ] **Step 3: Implement lifecycle**

At stream start, create the rollout and insert the activity/index. On normal finish, use `events_from_stream_capture` when a raw capture exists, otherwise `events_from_stream_snapshot`. Append `usage` and `session_end`. On errors, append `Error` and `SessionEnd`; do not delete `.part` files.

- [ ] **Step 4: Replace all streaming call sites**

Pass request events from the route into `record_stream_with_rollout`. For native Chat, use `events_from_chat_request`; for Responses bridge paths, use `events_from_internal_request`.

- [ ] **Step 5: Run stream tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --lib observability::stream_stats`

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test v1 responses::tests::streaming`

Expected: all pass.

## Remove Activity Contents and Old Recovery

**Task ID:** 8

**Files:**
- Delete: `src/activity/config.rs`
- Delete: `src/activity/content.rs`
- Delete: `src/storage/activity_contents.rs`
- Delete: `src/entity/activity_contents.rs`
- Delete: `src/schema/tables/activity_contents.rs`
- Delete: `src/schema/activity_content.rs`
- Delete: `tests/activity_contents.rs`
- Modify: `src/activity/mod.rs`
- Modify: `src/storage/mod.rs`
- Modify: `src/entity/mod.rs`
- Modify: `src/schema/mod.rs`
- Modify: `src/schema/tables/mod.rs`
- Modify: `src/storage/identities.rs`
- Modify: `src/identity/cleanup.rs`
- Modify: `src/main.rs`
- Create: `tests/activity_rollouts.rs`

**Interfaces:**
- Consumes: `activity_rollouts.rollout_path`.
- Produces: no runtime reads or writes of `activity_contents`; identity cleanup removes rollout files.

- [ ] **Step 1: Write the failing removal test**

Initialize an existing database with `activity_contents`, run `initialize`, and assert the table is gone. Assert `activity_rollouts` remains. Add a cleanup test that creates an identity, activity and rollout file, calls cleanup, and asserts the file is removed.

- [ ] **Step 2: Run failing tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test schema`

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test activity_rollouts`

Expected: fail while the old table and recovery code remain.

- [ ] **Step 3: Remove code**

Remove `activity_contents` entity/table/storage/config/recovery references. Remove `ACTIVITY_CONTENT_MAX_BYTES` initialization from `main` and the 5-minute recovery loop. Keep `redact_sensitive_text` and the event builders.

- [ ] **Step 4: Update cleanup**

Before deleting activities, collect `activity_rollouts.rollout_path` for affected identities. Commit the database transaction, then remove each file with `RolloutStore::absolute_path`. A missing file is logged and does not fail cleanup; a permission error is logged and does not roll back the identity deletion.

Change the cleanup interfaces to:

```rust
pub struct DeleteInactiveResult {
    pub deleted: u64,
    pub rollout_paths: Vec<String>,
}

impl Storage {
    pub async fn delete_inactive_identities(
        &self,
        now: DateTime<Utc>,
        retention: Duration,
    ) -> Result<DeleteInactiveResult, StorageError>;
}

pub async fn delete_expired_identities(
    storage: &Storage,
    rollout_store: &RolloutStore,
) -> Result<u64, StorageError>;
```

Update `main` and `tests/identity/cleanup.rs` for the new signature.

- [ ] **Step 5: Run tests**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test schema`

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test activity_rollouts`

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --test identity`

Expected: all pass.

## Documentation and Deployment

**Task ID:** 9

**Files:**
- Modify: `README.md`
- Modify: `deploy/docker-compose.yml`
- Modify: `docs/superpowers/specs/2026-08-31-activity-memory-design.md`

- [ ] **Step 1: Update README**

Remove `ACTIVITY_CONTENT_MAX_BYTES`. Document:

- `ACTIVITY_ROLLOUT_DIR`, default `data/sessions`
- `sessions/YYYY/MM/DD/rollout-*.jsonl`
- no truncation and no archive
- database only stores index
- `PROTOCOL_MAX_REQUEST_BODY_BYTES` remains 256 MiB

- [ ] **Step 2: Update Compose**

Mount the rollout directory:

```yaml
volumes:
  - ./data/sessions:/app/data/sessions
```

Do not modify `deploy/.env.example`.

- [ ] **Step 3: Mark the old activity-content spec**

Add a short note at the top of `2026-08-31-activity-memory-design.md` that its `activity_contents` storage section is superseded by `2026-10-22-activity-rollout-storage-design.md`. Keep the memory and source design unchanged.

- [ ] **Step 4: Check docs**

Run: `git diff --check`

Expected: no whitespace errors.

## Full Verification

**Task ID:** 10

**Files:** none.

- [ ] **Step 1: Format**

Run: `cargo fmt --all`

Run: `cargo fmt --all -- --check`

- [ ] **Step 2: Strict Clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`

- [ ] **Step 3: Full test suite with a dedicated PostgreSQL test database**

Run: `$env:TEST_POSTGRES_URL = "postgresql://<user>:<password>@<host>:5432/<test-database>"; cargo test --all-targets --all-features`

- [ ] **Step 4: Source layout and diff**

Run: `cargo test --test source_layout`

Run: `git diff --check`

- [ ] **Step 5: Verify the original symptom**

Create a streaming Responses request, let it complete with at least one tool call, then query:

```sql
SELECT activity_id, rollout_path, status, event_count, byte_size
FROM activity_rollouts
ORDER BY created_at DESC
LIMIT 1;
```

Open the referenced JSONL and assert it contains:

- `session_meta`
- a request `message`
- a `tool_call` with full arguments
- a response `message`
- `usage`
- `session_end`

Confirm that no `activity_contents` table exists and no file is truncated at 64 KiB.

## Self-Review

- Spec coverage: file source, event schema, index table, no archive, no truncation, redaction, memory boundary, schema deletion, cleanup and deployment are all assigned to tasks.
- Type consistency: `RolloutStore`, `RolloutWriter`, `RolloutSummary`, `RolloutEventBody` and the storage method names are defined before use.
- Boundary check: no task adds `archived_sessions`, a database content copy, or a memory extraction worker.

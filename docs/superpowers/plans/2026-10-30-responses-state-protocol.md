# Responses 状态协议 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `/v1/responses` 的 `store`、`previous_response_id`、响应 ID 和续接状态严格遵循 OpenAI Responses API 语义。

**Architecture:** 把 `identity_response_sessions` 从内部消息缓存升级为按身份隔离的精确 Responses item 状态表。Prelay 在请求进入上游前生成 `resp_` ID；续接时加载父响应的精确 input/output items，重新构造本轮完整输入。Conversations API 是独立子系统，本阶段只做显式拒绝。

**Tech Stack:** Rust 2021、Axum 0.7、SeaORM 2、PostgreSQL、serde_json、AES-GCM、chrono。

**Spec:** `docs/superpowers/specs/2026-10-30-responses-state-protocol.md`

## Global Constraints

- 官方语义以 [Conversation state](https://developers.openai.com/api/docs/guides/conversation-state) 和 [Create a response](https://developers.openai.com/api/reference/resources/responses/methods/create) 为准。
- `response_id` 必须由 Prelay 生成，格式为 `resp_<uuid-simple>`；上游 ID 只写入 `upstream_response_id`。
- `previous_response_id` 与 `conversation` 互斥；`conversation` 本阶段必须返回 `unsupported_parameter`，不能透传或忽略。
- `store` 缺省为 `true`；`store=true` 的普通响应保存 30 天；`store=false` 不写状态，但仍返回本地 `resp_` ID。
- 续接权威来源是精确 Responses `input_items` / `output_items`，不是 `InternalMessage` / `InternalOutputItem`。
- `instructions` 只作用于当前请求；父响应的 instructions 不进入续接 items。
- 只有 `status='completed'`、`protocol_format='responses_items_v1'` 且未过期、属于当前 identity 的状态可以作为父响应。
- 上游请求必须移除 `previous_response_id`、`conversation`，并把 `store` 置为 `false`。
- 父响应不存在、跨身份、已过期、未完成或 legacy 不可续接时，统一返回 HTTP 400 和 `previous_response_not_found`，不能泄露其他身份是否存在该响应。
- `/v1/responses` 的协议错误使用 OpenAI 错误体：`error.type`、`error.code`、`error.param`、`error.message`。
- 流式只改写 `response.id` 和顶层 `response_id`；`item.id`、`item_id`、`call_id` 保持原值。
- 单个 Rust 文件不超过 450 行；新增职责优先拆到独立模块，不提高 `tests/source_layout` 门禁。
- 数据库测试必须使用 `TEST_POSTGRES_URL` 指向的专用测试库；不得使用生产库。
- 当前核对的部署库中 `identity_response_sessions` 为空；不安排数据回填、备份或批量删除，不要删除或清空该表。
- 当前实际流量以 Codex 的流式 `/v1/responses` 为主，客户端发送 `store=false`，因此实现完成后该表仍可能保持为空；不得用表空判断实现失败，验证写入必须使用带 `store=true` 的测试请求。
- 每个 Task 先写失败测试，再做最小实现，定向测试通过后执行 `cargo fmt --all`、严格 Clippy、`git diff --check`，最后单独提交。
- 不推送、不打标签、不发布、不部署；这些动作分别需要用户明确授权。

---

## File Structure

- `src/error.rs`：新增 OpenAI 风格错误变体和响应体。
- `src/codec/bridge/responses/decode.rs`：校验状态参数；不把协议 `conversation` 塞进内部请求。
- `src/inbound/v1/responses/handler.rs`：生成 Prelay 响应 ID；统一加载父状态。
- `src/inbound/v1/responses/state.rs`：当前输入 items、父链拼接、精确 item 到 Chat 消息的严格转换。
- `src/inbound/v1/responses/candidate.rs`：按上游协议选择 native 或 Chat 桥接，并传递精确 items。
- `src/inbound/v1/responses/native.rs`：原生 Responses 直通、ID 改写、精确状态落库。
- `src/inbound/v1/responses/chat.rs`：Chat 桥接、精确状态落库。
- `src/codec/providers/responses/request.rs`：构造发往上游的精确 payload，移除协议状态字段。
- `src/codec/bridge/stream/events.rs`：流式统计保存上游 ID 和完整 output items。
- `src/codec/bridge/stream/decode/responses.rs`：捕获 `response.output`。
- `src/inbound/v1/responses/stream_rewrite.rs`：按 SSE 事件改写本地响应 ID。
- `src/service/recording/state/session.rs`：流结束后用本地 ID 和精确 items 落库。
- `src/store/schema/tables/sessions.rs`、`src/store/schema/indexes.rs`、`src/store/schema/mod.rs`：新增列、索引、迁移和外键移除。
- `src/store/entity/identity/response_sessions.rs`：SeaORM 字段和关系。
- `src/store/storage/sessions.rs`：精确状态读写、加密、TTL 删除。
- `src/bootstrap/tasks.rs`、`src/bootstrap/serve.rs`：启动清理和每日清理。
- `docs/superpowers/specs/2026-10-30-responses-state-protocol.md`：随实现更新边界和错误契约。

---

### Task 1: 协议参数校验与 OpenAI 错误体

**Files:**
- Modify: `src/error.rs`
- Modify: `src/codec/bridge/responses/decode.rs`
- Modify: `src/codec/bridge/responses/tests.rs`

**Interfaces:**
- Produces: `AppError::OpenAi { status, error_type, code, param, message }`。
- Produces: `AppError::openai_invalid(param, message)` 和 `AppError::openai_unsupported(param, message)`。
- Produces: `decode_responses_request` 校验 `conversation`、`store`、`previous_response_id`、`stream` 的类型。
- 不新增 `InternalRequest.conversation`；`conversation` 在解码阶段就被拒绝，不能进入后续链路。

- [ ] **Step 1: 写失败测试**

在 `src/codec/bridge/responses/tests.rs` 增加：

```rust
use crate::error::AppError;

#[test]
fn rejects_previous_response_and_conversation_together() {
    let error = decode_responses_request(&json!({
        "model": "gpt-test",
        "previous_response_id": "resp_parent",
        "conversation": "conv_1",
        "input": "hello"
    }))
    .expect_err("mutually exclusive parameters must fail");

    let AppError::OpenAi {
        status,
        error_type,
        code,
        param,
        ..
    } = error
    else {
        panic!("expected OpenAI-style error");
    };
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(error_type, "invalid_request_error");
    assert_eq!(code, "invalid_request_error");
    assert_eq!(param, Some("conversation"));
}

#[test]
fn rejects_conversation_string_and_object_as_unsupported() {
    for conversation in [
        json!("conv_1"),
        json!({ "id": "conv_1" }),
    ] {
        let error = decode_responses_request(&json!({
            "model": "gpt-test",
            "conversation": conversation,
            "input": "hello"
        }))
        .expect_err("conversation is not implemented yet");

        let AppError::OpenAi { code, param, .. } = error else {
            panic!("expected OpenAI-style error");
        };
        assert_eq!(code, "unsupported_parameter");
        assert_eq!(param, Some("conversation"));
    }
}

#[test]
fn defaults_store_to_true_and_rejects_non_boolean_store() {
    let request = decode_responses_request(&json!({
        "model": "gpt-test",
        "input": "hello"
    }))
    .expect("decode request");
    assert!(request.store);

    let error = decode_responses_request(&json!({
        "model": "gpt-test",
        "input": "hello",
        "store": "false"
    }))
    .expect_err("store must be a boolean or null");
    assert!(matches!(
        error,
        AppError::OpenAi {
            param: Some("store"),
            ..
        }
    ));
}
```

在 `src/error.rs` 的测试模块增加错误体形状测试：

```rust
#[tokio::test]
async fn openai_error_uses_the_protocol_envelope() {
    let response = AppError::OpenAi {
        status: StatusCode::BAD_REQUEST,
        error_type: "invalid_request_error",
        code: "unsupported_parameter",
        param: Some("conversation"),
        message: "conversation is not supported yet".to_string(),
    }
    .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read error response");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("JSON error"),
        json!({
            "error": {
                "type": "invalid_request_error",
                "code": "unsupported_parameter",
                "param": "conversation",
                "message": "conversation is not supported yet"
            }
        })
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run:

```text
cargo test --lib rejects_previous_response_and_conversation_together -- --nocapture
cargo test --lib openai_error_uses_the_protocol_envelope -- --nocapture
```

Expected: FAIL，`AppError::OpenAi` 或 `conversation` 校验不存在。

- [ ] **Step 3: 实现错误变体**

在 `src/error.rs` 增加：

```rust
OpenAi {
    status: StatusCode,
    error_type: &'static str,
    code: &'static str,
    param: Option<&'static str>,
    message: String,
},
```

在 `IntoResponse` 的匹配中先处理该变体：

```rust
AppError::OpenAi {
    status,
    error_type,
    code,
    param,
    message,
} => (
    status,
    json!({
        "type": error_type,
        "code": code,
        "param": param,
        "message": message,
    }),
),
```

保留现有 `{"error": ...}` 外层包装。增加构造方法：

```rust
pub(crate) fn openai_invalid(param: &'static str, message: impl Into<String>) -> Self {
    Self::OpenAi {
        status: StatusCode::BAD_REQUEST,
        error_type: "invalid_request_error",
        code: "invalid_request_error",
        param: Some(param),
        message: message.into(),
    }
}

pub(crate) fn openai_unsupported(param: &'static str, message: impl Into<String>) -> Self {
    Self::OpenAi {
        status: StatusCode::BAD_REQUEST,
        error_type: "invalid_request_error",
        code: "unsupported_parameter",
        param: Some(param),
        message: message.into(),
    }
}
```

在 `src/codec/bridge/responses/decode.rs` 中：

1. 解析 `previous_response_id` 时，只接受 `null` 或非空字符串；其他类型返回 `openai_invalid("previous_response_id", ...)`。
2. 解析 `conversation`：

```rust
fn decode_conversation(value: Option<&Value>) -> Result<Option<String>, AppError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if !id.trim().is_empty() => Ok(Some(id.clone())),
        Some(Value::Object(object)) => object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .map(Some)
            .ok_or_else(|| AppError::openai_invalid(
                "conversation",
                "conversation must be a string or an object with a non-empty id",
            )),
        Some(_) => Err(AppError::openai_invalid(
            "conversation",
            "conversation must be a string or an object with a non-empty id",
        )),
    }
}
```

3. 先检查互斥，再检查未实现：

```rust
if previous_response_id.is_some() && conversation.is_some() {
    return Err(AppError::openai_invalid(
        "conversation",
        "previous_response_id and conversation cannot be used together",
    ));
}
if conversation.is_some() {
    return Err(AppError::openai_unsupported(
        "conversation",
        "conversation is not supported yet",
    ));
}
```

4. `store` 只接受 `null` 或布尔值，缺省为 `true`；`stream` 只接受 `null` 或布尔值。类型错误返回 OpenAI 错误体。

- [ ] **Step 4: 运行测试确认通过**

Run:

```text
cargo test --lib responses::tests -- --nocapture
cargo test --lib error::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add src/error.rs src/codec/bridge/responses/decode.rs src/codec/bridge/responses/tests.rs
git commit -m "按 Responses 规范校验状态参数"
```

---

### Task 2: 精确状态 schema 与迁移

**Files:**
- Modify: `src/store/schema/tables/sessions.rs`
- Modify: `src/store/schema/indexes.rs`
- Modify: `src/store/schema/mod.rs`
- Modify: `src/store/schema/validation.rs`
- Modify: `tests/schema/contract.rs`
- Modify: `tests/schema.rs`
- Create: `tests/schema/response_state_migration.rs`

**Interfaces:**
- Produces: `identity_response_sessions` 新列 `status`、`protocol_format`、`upstream_response_id`、`request_items_json`、`response_items_json`、`expires_at`。
- Produces: 索引 `idx_identity_response_sessions_expires_at`。
- Produces: `provider_id` 外键被移除，但列和值保留为元数据。
- 当前表为空：本 Task 只做结构迁移，不安排数据回填或清理。

- [ ] **Step 1: 写迁移测试**

创建 `tests/schema/response_state_migration.rs`：

```rust
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use prelay_server::{store::schema::initialize, test_support::test_empty_database_connection};

async fn column_exists(db: &DatabaseConnection, table: &str, column: &str) -> bool {
    let sql = format!(
        "SELECT COUNT(*) AS result_count FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = '{table}' \
         AND column_name = '{column}'"
    );
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .expect("query columns")
        .expect("column count row");
    row.try_get::<i64>("", "result_count").expect("count") == 1
}

async fn index_exists(db: &DatabaseConnection, table: &str, index: &str) -> bool {
    let sql = format!(
        "SELECT COUNT(*) AS result_count FROM pg_indexes \
         WHERE schemaname = current_schema() AND tablename = '{table}' \
         AND indexname = '{index}'"
    );
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .expect("query indexes")
        .expect("index count row");
    row.try_get::<i64>("", "result_count").expect("count") == 1
}

async fn provider_fk_exists(db: &DatabaseConnection) -> bool {
    let sql = "SELECT COUNT(*) AS result_count
               FROM pg_constraint con
               JOIN pg_class rel ON rel.oid = con.conrelid
               JOIN pg_namespace nsp ON nsp.oid = rel.relnamespace
               WHERE nsp.nspname = current_schema()
                 AND rel.relname = 'identity_response_sessions'
                 AND con.contype = 'f'
                 AND pg_get_constraintdef(con.oid) LIKE 'FOREIGN KEY (provider_id)%'";
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .expect("query constraints")
        .expect("constraint count row");
    row.try_get::<i64>("", "result_count").expect("count") > 0
}

#[tokio::test]
async fn upgrades_response_state_schema_and_removes_provider_fk() {
    let db = test_empty_database_connection().await;
    initialize(&db).await.expect("initialize current schema");

    db.execute_unprepared(
        "ALTER TABLE identity_response_sessions
         DROP COLUMN request_items_json,
         DROP COLUMN response_items_json,
         DROP COLUMN upstream_response_id,
         DROP COLUMN status,
         DROP COLUMN protocol_format,
         DROP COLUMN expires_at",
    )
    .await
    .expect("simulate previous schema");
    db.execute_unprepared(
        "ALTER TABLE identity_response_sessions
         ADD CONSTRAINT identity_response_sessions_provider_id_fkey
         FOREIGN KEY (provider_id) REFERENCES identity_provider_configs(id)",
    )
    .await
    .expect("restore legacy provider foreign key");

    initialize(&db).await.expect("upgrade schema");

    for column in [
        "status",
        "protocol_format",
        "upstream_response_id",
        "request_items_json",
        "response_items_json",
        "expires_at",
    ] {
        assert!(
            column_exists(&db, "identity_response_sessions", column).await,
            "missing column {column}"
        );
    }
    assert!(
        index_exists(
            &db,
            "identity_response_sessions",
            "idx_identity_response_sessions_expires_at"
        )
        .await
    );
    assert!(!provider_fk_exists(&db).await);
}
```

在 `tests/schema.rs` 注册模块：

```rust
#[path = "schema/response_state_migration.rs"]
mod response_state_migration;
```

- [ ] **Step 2: 运行测试确认失败**

```text
cargo test --test schema response_state_migration -- --nocapture
```

Expected: FAIL，列、索引或迁移不存在。

- [ ] **Step 3: 更新表定义和迁移**

在 `src/store/schema/tables/sessions.rs` 的建表语句中增加：

```rust
.col(
    ColumnDef::new(ResponseSessions::Status)
        .string()
        .not_null()
        .default("legacy"),
)
.col(
    ColumnDef::new(ResponseSessions::ProtocolFormat)
        .string()
        .not_null()
        .default("legacy_internal_v1"),
)
.col(ColumnDef::new(ResponseSessions::UpstreamResponseId).string())
.col(ColumnDef::new(ResponseSessions::RequestItemsJson).string())
.col(ColumnDef::new(ResponseSessions::ResponseItemsJson).string())
.col(ColumnDef::new(ResponseSessions::ExpiresAt).string())
```

删除 `provider_id` 的 `ForeignKey`，保留：

```rust
.col(
    ColumnDef::new(ResponseSessions::ProviderId)
        .string()
        .not_null(),
)
```

在 `ResponseSessions` 枚举中加入六个新标识符。

在 `src/store/schema/indexes.rs` 增加：

```rust
pub(super) fn response_sessions_expires_at() -> IndexCreateStatement {
    Index::create()
        .name("idx_identity_response_sessions_expires_at")
        .table(ResponseSessions::Table)
        .col(ResponseSessions::ExpiresAt)
        .if_not_exists()
        .to_owned()
}
```

在 `initialize_empty_schema` 中创建该索引；在 `migrate_response_sessions_schema` 中增加幂等列迁移和索引创建：

```sql
ALTER TABLE identity_response_sessions ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE identity_response_sessions ADD COLUMN IF NOT EXISTS protocol_format TEXT NOT NULL DEFAULT 'legacy_internal_v1';
ALTER TABLE identity_response_sessions ADD COLUMN IF NOT EXISTS upstream_response_id TEXT;
ALTER TABLE identity_response_sessions ADD COLUMN IF NOT EXISTS request_items_json TEXT;
ALTER TABLE identity_response_sessions ADD COLUMN IF NOT EXISTS response_items_json TEXT;
ALTER TABLE identity_response_sessions ADD COLUMN IF NOT EXISTS expires_at TEXT;
```

用下面的 DO 块按定义删除 `provider_id` 外键，不依赖自动生成的约束名：

```sql
DO $$
DECLARE
    fk_name text;
BEGIN
    SELECT con.conname
      INTO fk_name
      FROM pg_constraint con
      JOIN pg_class rel ON rel.oid = con.conrelid
      JOIN pg_namespace nsp ON nsp.oid = rel.relnamespace
     WHERE nsp.nspname = current_schema()
       AND rel.relname = 'identity_response_sessions'
       AND con.contype = 'f'
       AND pg_get_constraintdef(con.oid) LIKE 'FOREIGN KEY (provider_id)%'
     LIMIT 1;

    IF fk_name IS NOT NULL THEN
        EXECUTE format(
            'ALTER TABLE identity_response_sessions DROP CONSTRAINT %I',
            fk_name
        );
    END IF;
END
$$;
```

更新 `src/store/schema/validation.rs` 的 `response_session_schema_exists`，要求新列存在，并检查 TTL 索引；不要要求 provider 外键存在。

- [ ] **Step 4: 更新 schema contract 并运行测试**

在 `tests/schema/contract.rs` 增加新列的类型断言和 provider 外键不存在断言。Run:

```text
cargo test --test schema -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add src/store/schema/tables/sessions.rs src/store/schema/indexes.rs src/store/schema/mod.rs src/store/schema/validation.rs tests/schema.rs tests/schema/contract.rs tests/schema/response_state_migration.rs
git commit -m "补充 Responses 精确状态存储结构"
```

---

### Task 3: 精确状态读写 API

**Files:**
- Modify: `src/store/entity/identity/response_sessions.rs`
- Modify: `src/store/storage/mod.rs`
- Modify: `src/store/storage/sessions.rs`

**Interfaces:**
- Produces: `ResponseState`、`ResponseStateInsert<'a>`。
- Produces: `Storage::save_response_state`、`Storage::load_response_state`、`Storage::delete_expired_response_states`。
- Produces: `StorageError::InvalidStoredState(String)`，归类为内部错误。

- [ ] **Step 1: 写失败测试**

在 `src/store/storage/sessions.rs` 的测试模块增加：

```rust
#[tokio::test]
async fn stores_and_loads_exact_response_items() {
    let storage = test_storage().await;
    let (identity_id, provider_id) = seed_identity_and_provider(&storage, "exact").await;
    let request_items = json!([
        {"role": "user", "content": "hello"}
    ]);
    let response_items = json!([
        {
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "hi"}]
        }
    ]);

    storage
        .save_response_state(ResponseStateInsert {
            identity_id: &identity_id,
            response_id: "resp_local",
            previous_response_id: None,
            conversation_id: "conv_local",
            provider_id: &provider_id,
            model: "model-local",
            upstream_response_id: Some("resp_upstream"),
            request_items: &request_items,
            response_items: &response_items,
            input_messages: &[],
            response: &response("resp_local", "hi"),
        })
        .await
        .expect("save exact state");

    let state = storage
        .load_response_state(&identity_id, "resp_local")
        .await
        .expect("load exact state")
        .expect("state exists");
    assert_eq!(state.request_items, request_items);
    assert_eq!(state.response_items, response_items);
    assert_eq!(state.upstream_response_id.as_deref(), Some("resp_upstream"));
}

#[tokio::test]
async fn does_not_load_legacy_or_expired_state() {
    let storage = test_storage().await;
    let (identity_id, provider_id) = seed_identity_and_provider(&storage, "legacy").await;

    identity_response_sessions::ActiveModel {
        response_id: Set("resp_legacy".to_string()),
        identity_id: Set(identity_id.clone()),
        previous_response_id: Set(None),
        conversation_id: Set(None),
        provider_id: Set(provider_id.clone()),
        model: Set("model".to_string()),
        input_messages_json: Set("[]".to_string()),
        output_items_json: Set("[]".to_string()),
        status: Set("legacy".to_string()),
        protocol_format: Set("legacy_internal_v1".to_string()),
        upstream_response_id: Set(None),
        request_items_json: Set(None),
        response_items_json: Set(None),
        expires_at: Set(None),
        created_at: Set("2026-01-01T00:00:00Z".to_string()),
    }
    .insert(&storage.db)
    .await
    .expect("insert legacy row");

    assert!(
        storage
            .load_response_state(&identity_id, "resp_legacy")
            .await
            .expect("load legacy")
            .is_none()
    );

    identity_response_sessions::ActiveModel {
        response_id: Set("resp_expired".to_string()),
        identity_id: Set(identity_id.clone()),
        previous_response_id: Set(None),
        conversation_id: Set(None),
        provider_id: Set(provider_id),
        model: Set("model".to_string()),
        input_messages_json: Set("[]".to_string()),
        output_items_json: Set("[]".to_string()),
        status: Set("completed".to_string()),
        protocol_format: Set("responses_items_v1".to_string()),
        upstream_response_id: Set(None),
        request_items_json: Set(Some("[]".to_string())),
        response_items_json: Set(Some("[]".to_string())),
        expires_at: Set(Some("2000-01-01T00:00:00Z".to_string())),
        created_at: Set("2026-01-01T00:00:00Z".to_string()),
    }
    .insert(&storage.db)
    .await
    .expect("insert expired exact row");

    assert!(
        storage
            .load_response_state(&identity_id, "resp_expired")
            .await
            .expect("load expired")
            .is_none()
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

```text
cargo test --lib stores_and_loads_exact_response_items -- --nocapture
cargo test --lib does_not_load_legacy_or_expired_state -- --nocapture
```

Expected: FAIL，类型或方法不存在。

- [ ] **Step 3: 实现实体和存储 API**

在 `src/store/entity/identity/response_sessions.rs` 增加字段：

```rust
pub status: String,
pub protocol_format: String,
pub upstream_response_id: Option<String>,
pub request_items_json: Option<String>,
pub response_items_json: Option<String>,
pub expires_at: Option<String>,
```

删除到 `provider_configs` 的 `Provider` 关系和 `Related` 实现；`Identity` 关系保留。

在 `src/store/storage/sessions.rs` 增加：

```rust
pub struct ResponseState {
    pub identity_id: String,
    pub response_id: String,
    pub previous_response_id: Option<String>,
    pub conversation_id: Option<String>,
    pub provider_id: String,
    pub model: String,
    pub upstream_response_id: Option<String>,
    pub request_items: Value,
    pub response_items: Value,
    pub created_at: String,
    pub expires_at: String,
}

pub struct ResponseStateInsert<'a> {
    pub identity_id: &'a str,
    pub response_id: &'a str,
    pub previous_response_id: Option<&'a str>,
    pub conversation_id: &'a str,
    pub provider_id: &'a str,
    pub model: &'a str,
    pub upstream_response_id: Option<&'a str>,
    pub request_items: &'a Value,
    pub response_items: &'a Value,
    pub input_messages: &'a [InternalMessage],
    pub response: &'a InternalResponse,
}
```

`save_response_state` 的行为：

1. 用 `encrypt_session_value` 加密 `request_items`、`response_items`、`input_messages`、`response.output`。
2. 写入 `status='completed'`、`protocol_format='responses_items_v1'`。
3. `created_at` 使用 `Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)`。
4. `expires_at` 使用同一格式的 `created_at + Duration::days(30)`。
5. 保留 upsert；冲突时更新所有状态列和派生列。

`load_response_state` 的行为：

1. 按 `(response_id, identity_id)` 查询。
2. `protocol_format != "responses_items_v1"` 或 `status != "completed"` 返回 `Ok(None)`。
3. `expires_at` 为空或 `<= now` 返回 `Ok(None)`。
4. `request_items_json` / `response_items_json` 为空返回 `StorageError::InvalidStoredState`。
5. 解密、反序列化为 `Value`；损坏时返回 `StorageError::InvalidStoredState`，不能伪装成“父响应不存在”。

`delete_expired_response_states` 删除 `expires_at IS NOT NULL AND expires_at < now` 的行，返回删除行数；不要删除 `expires_at` 为空的 legacy 行。

在 `src/store/storage/mod.rs` 导出新类型和方法。

- [ ] **Step 4: 运行存储测试**

```text
cargo test --lib store::storage::sessions::tests:: -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add src/store/entity/identity/response_sessions.rs src/store/storage/mod.rs src/store/storage/sessions.rs
git commit -m "增加 Responses 精确状态读写"
```

---

### Task 4: 统一 Prelay response_id

**Files:**
- Modify: `src/inbound/v1/responses/handler.rs`
- Modify: `src/inbound/v1/responses/candidate.rs`
- Modify: `src/inbound/v1/responses/native.rs`
- Modify: `src/inbound/v1/responses/chat.rs`
- Modify: `src/inbound/v1/responses/tests/routing.rs`
- Modify: `src/inbound/v1/responses/tests/sessions.rs`

**Interfaces:**
- Produces: `ResponseCandidateRequest.response_id: String`。
- Produces: `ResponseBridgeContext.response_id: String`。
- `create_native_response` 和 `create_chat_response` 接收本地 ID；客户端看到的 `id` 始终是 `resp_<uuid-simple>`。

- [ ] **Step 1: 写失败测试**

在 `src/inbound/v1/responses/tests/sessions.rs` 增加 native 非流式测试，使用现有 `create_response`、`spawn_native_responses_upstream`、`create_test_endpoint_auth` 和 `response_json` 模式：

```rust
#[tokio::test]
async fn native_non_streaming_returns_a_local_response_id() {
    let upstream = spawn_native_responses_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider("gpt-4.1", "openai", &upstream, "sk-upstream")
        .await
        .expect("create provider");
    let auth = create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;

    let response = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "gpt-4.1",
            "input": "hello",
            "store": true
        })),
    )
    .await
    .expect("create response");
    let body = response_json(response).await;
    let id = body["id"].as_str().expect("response id");

    assert!(id.starts_with("resp_"));
    assert_ne!(id, "resp_native");
}
```

现有 chat 测试继续断言本地 `resp_` 前缀。

- [ ] **Step 2: 运行测试确认失败**

```text
cargo test --lib native_non_streaming_returns_a_local_response_id -- --nocapture
```

Expected: FAIL，native 返回上游 `resp_upstream`。

- [ ] **Step 3: 生成并传递本地 ID**

在 `handler.rs` 生成：

```rust
let response_id = format!("resp_{}", uuid::Uuid::new_v4().simple());
```

把 `response_id` 放进 `ResponseCandidateRequest`，再放进 `ResponseBridgeContext`。`chat.rs` 删除内部 `format!("resp_...")`，使用传入值。`native.rs` 在读取上游 JSON 后：

```rust
let upstream_response_id = response
    .get("id")
    .and_then(Value::as_str)
    .map(str::to_string);
let mut response = response;
response["id"] = json!(context.response_id);
let decoded_response = decode_responses_response(response.clone())?;
```

本 Task 只统一 ID；精确状态写入在 Task 5 替换。保存旧派生列时使用本地 `decoded_response.id`。

- [ ] **Step 4: 运行 native 和 chat 回归**

```text
cargo test --lib responses::tests -- --nocapture
```

Expected: PASS；native 和 chat 都返回本地 ID。

- [ ] **Step 5: 提交**

```powershell
git add src/inbound/v1/responses/handler.rs src/inbound/v1/responses/candidate.rs src/inbound/v1/responses/native.rs src/inbound/v1/responses/chat.rs src/inbound/v1/responses/tests/routing.rs src/inbound/v1/responses/tests/sessions.rs
git commit -m "统一 Responses 的本地响应标识"
```

---

### Task 5: 用精确 items 续接 previous_response_id

**Files:**
- Create: `src/inbound/v1/responses/state.rs`
- Modify: `src/inbound/v1/responses/handler.rs`
- Modify: `src/inbound/v1/responses/candidate.rs`
- Modify: `src/inbound/v1/responses/native.rs`
- Modify: `src/inbound/v1/responses/chat.rs`
- Modify: `src/inbound/v1/responses/sessions.rs`
- Modify: `src/codec/providers/responses/request.rs`
- Modify: `src/codec/bridge/responses/decode.rs`
- Modify: `src/inbound/v1/responses/tests/sessions.rs`
- Modify: `src/inbound/v1/responses/tests/fixtures.rs`

**Interfaces:**
- Produces: `current_input_items(payload: &Value) -> Result<Vec<Value>, AppError>`。
- Produces: `continuation_items(parent: &ResponseState, current: &[Value]) -> Vec<Value>`。
- Produces: `decode_items_for_chat_bridge(items: &[Value]) -> Result<Vec<InternalMessage>, AppError>`。
- Produces: `encode_native_responses_request_body(payload, model, input_items: Option<&[Value]>) -> Vec<u8>`。
- `ResponseCandidateRequest` 增加 `parent_state: Option<Arc<ResponseState>>`。

- [ ] **Step 1: 写精确续接测试**

在 `src/inbound/v1/responses/tests/fixtures.rs` 增加一个捕获上游请求体的原生 Responses mock：

```rust
pub(super) async fn spawn_capturing_native_responses_upstream() -> (String, std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>) {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let state = captured.clone();
    let app = axum::Router::new().route(
        "/responses",
        axum::routing::post(move |axum::Json(payload): axum::Json<serde_json::Value>| {
            let state = state.clone();
            async move {
                state.lock().expect("capture upstream request").push(payload);
                axum::Json(json!({
                    "id": "resp_upstream",
                    "object": "response",
                    "status": "completed",
                    "model": "gpt-4.1",
                    "output": [{
                        "type": "message",
                        "id": "msg_upstream",
                        "status": "completed",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "ok", "annotations": []}]
                    }],
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let address = listener.local_addr().expect("upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    (format!("http://{address}"), captured)
}
```

在 `sessions.rs` 增加：

```rust
#[tokio::test]
async fn previous_response_replays_exact_items_and_removes_upstream_state_fields() {
    let (upstream, captured) = spawn_capturing_native_responses_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider("gpt-4.1", "openai", &upstream, "sk-upstream")
        .await
        .expect("create provider");
    let auth = create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;

    let first = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "gpt-4.1",
            "input": "first",
            "store": true
        })),
    )
    .await
    .expect("create first response");
    let first = response_json(first).await;
    let parent_id = first["id"].as_str().expect("parent id");

    let second = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "gpt-4.1",
            "previous_response_id": parent_id,
            "instructions": "new instructions",
            "input": "second",
            "store": true
        })),
    )
    .await
    .expect("create second response");
    let second = response_json(second).await;
    assert!(second["id"].as_str().expect("second id").starts_with("resp_"));

    let requests = captured.lock().expect("lock captured requests");
    let upstream_request = requests.last().expect("second upstream request");
    assert_eq!(upstream_request["input"][0]["content"], "first");
    assert_eq!(upstream_request["input"][1]["role"], "assistant");
    assert_eq!(upstream_request["input"][2]["content"], "second");
    assert!(upstream_request.get("previous_response_id").is_none());
    assert!(upstream_request.get("conversation").is_none());
    assert_eq!(upstream_request["store"], false);
    assert_eq!(upstream_request["instructions"], "new instructions");
}
```

再加三个测试，分别覆盖：

1. 不存在的父 ID：HTTP 层返回 400，`code=previous_response_not_found`。
2. 过期或 legacy 父行：返回同一 code，不泄露内部原因。
3. 跨身份父 ID：返回同一 code，且不触发跨身份读取。

- [ ] **Step 2: 运行测试确认失败**

```text
cargo test --lib previous_response_replays_exact_items_and_removes_upstream_state_fields -- --nocapture
```

Expected: FAIL，当前仍用内部消息历史，且上游可能收到 `store=true`。

- [ ] **Step 3: 实现精确状态流程**

在 `state.rs` 实现：

```rust
pub(super) fn current_input_items(payload: &Value) -> Result<Vec<Value>, AppError> {
    match payload.get("input") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(text)) => Ok(vec![json!({
            "role": "user",
            "content": text
        })]),
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(_) => Err(AppError::openai_invalid(
            "input",
            "input must be a string or an array of response items",
        )),
    }
}

pub(super) fn continuation_items(
    parent: &ResponseState,
    current: &[Value],
) -> Vec<Value> {
    let mut items = parent.request_items.clone();
    items.extend(parent.response_items.clone());
    items.extend(current.iter().cloned());
    items
}
```

`handler.rs` 在解析请求后加载一次父状态：

```rust
let parent_state = match request.previous_response_id.as_deref() {
    Some(response_id) => state
        .storage
        .load_response_state(&access.identity_id, response_id)
        .await?
        .map(std::sync::Arc::new),
    None => None,
};
if request.previous_response_id.is_some() && parent_state.is_none() {
    return Err(AppError::openai_previous_response_not_found());
}
```

`AppError` 增加构造方法：

```rust
pub(crate) fn openai_previous_response_not_found() -> Self {
    Self::OpenAi {
        status: StatusCode::BAD_REQUEST,
        error_type: "invalid_request_error",
        code: "previous_response_not_found",
        param: Some("previous_response_id"),
        message: "Previous response not found".to_string(),
    }
}
```

用父状态的 `conversation_id` 作为本轮活动分组；没有父状态时生成 `conv_<uuid-simple>`。删除 `request_with_session_history` 和 `conversation_id_for_request` 的旧历史读取逻辑，避免继续把 `InternalMessage` 当协议状态。

`candidate.rs`：

- native 分支始终构造精确 `input_items`：无父响应使用 `current_input_items`；有父响应使用 `continuation_items`。
- 调用新的上游 payload 编码器，传入 `Some(&items)`。
- chat 分支用 `decode_items_for_chat_bridge(&items)` 得到消息；转换失败返回 `AppError::openai_unsupported` 或 `AppError::openai_invalid`，不能静默丢弃 reasoning、图片或其他无法表达 item。
- 更新 `tests/fixtures.rs` 中 `spawn_native_responses_upstream` 和 `spawn_streaming_native_responses_upstream` 对 `payload["input"]` 的断言：输入字符串现在会以 Responses item 数组发送，断言精确数组而不是字符串。

`codec/providers/responses/request.rs` 把编码器改为：

```rust
pub fn encode_native_responses_request_body(
    payload: &Value,
    model: &str,
    input_items: Option<&[Value]>,
) -> Vec<u8>
```

序列化时：

- 跳过 `previous_response_id`、`conversation`。
- 顶层 `store` 固定写 `false`。
- `model` 替换为上游模型名。
- 有 `input_items` 时写 `"input": input_items`；没有时保留原 payload 的 `input`。
- 继续使用自定义 `Serialize` 包装器，避免为几十 MB 的图片 payload 深拷贝。

`decode_items_for_chat_bridge` 只接受：

- `message`，角色 `user`/`assistant`/`system`/`developer`，内容为文本或图片；
- `function_call`；
- `function_call_output`。

其他 item 类型（包括 `reasoning`、内置工具调用）返回明确错误，不能降级成用户消息。

- [ ] **Step 4: 运行续接和桥接测试**

```text
cargo test --lib responses::tests -- --nocapture
```

Expected: PASS；父链 items 顺序、instructions、store=false、错误 code 都符合规范。

- [ ] **Step 5: 提交**

```powershell
git add src/inbound/v1/responses/state.rs src/inbound/v1/responses/handler.rs src/inbound/v1/responses/candidate.rs src/inbound/v1/responses/native.rs src/inbound/v1/responses/chat.rs src/inbound/v1/responses/sessions.rs src/codec/providers/responses/request.rs src/codec/bridge/responses/decode.rs src/inbound/v1/responses/tests/sessions.rs src/inbound/v1/responses/tests/fixtures.rs
git commit -m "按精确 Responses items 续接父响应"
```

---

### Task 6: store=false 与完成态落库

**Files:**
- Modify: `src/inbound/v1/responses/native.rs`
- Modify: `src/inbound/v1/responses/chat.rs`
- Modify: `src/service/recording/state/session.rs`
- Modify: `src/service/recording/options.rs`
- Modify: `src/inbound/v1/responses/tests/sessions.rs`
- Modify: `src/service/recording/tests/session.rs`
- Modify: `src/service/activity.rs`
- Modify: `src/store/storage/mod.rs`
- Modify: `tests/identity/storage/sessions.rs`
- Modify: `tests/identity/cleanup.rs`
- Modify: `tests/v1/identity_scope.rs`

**Interfaces:**
- Produces: `save_response_state` 只在 `request.store == true` 且响应完成时调用。
- Produces: `StreamSession.response_id: String`、`request_items: Vec<Value>`、`upstream_response_id: Option<String>`。
- `store=false` 不写 `identity_response_sessions`；该 ID 再次作为父响应时返回 `previous_response_not_found`。

- [ ] **Step 1: 写失败测试**

在 `src/inbound/v1/responses/tests/sessions.rs` 增加：

```rust
#[tokio::test]
async fn store_false_does_not_persist_a_continuable_response() {
    let upstream = spawn_history_asserting_chat_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider("deepseek-chat", "openai_compatible", &upstream, "sk-upstream")
        .await
        .expect("create provider");
    let auth = create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat").await;

    let first = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello",
            "store": false
        })),
    )
    .await
    .expect("create response");
    let first = response_json(first).await;
    let id = first["id"].as_str().expect("response id");

    let error = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "deepseek-chat",
            "previous_response_id": id,
            "input": "again"
        })),
    )
    .await
    .expect_err("unstored response cannot be continued");

    assert!(matches!(
        error,
        crate::error::AppError::OpenAi {
            code: "previous_response_not_found",
            ..
        }
    ));
}
```

- [ ] **Step 2: 运行测试确认失败**

```text
cargo test --lib store_false_does_not_persist_a_continuable_response -- --nocapture
```

Expected: FAIL，当前可能仍写状态。

- [ ] **Step 3: 收紧落库条件并迁移调用方**

1. native 非流式：从原始上游 JSON 取 `output` 数组作为 `response_items`；把 `request_items`（本轮实际发送的完整 items）传给 `save_response_state`。
2. chat 非流式：先用 `encode_responses_response(response.clone())` 得到协议响应，再取 `output` 作为 `response_items`；`request_items` 使用本轮精确 items。
3. 两个非流式路径都只在 `request.store` 为 true 时保存。
4. `StreamSession` 改为保存 `response_id` 本地 ID、`request_items`、`upstream_response_id`；流完成时用这些字段保存。
5. 流式保存失败继续记录错误日志，但不能把未保存的 ID 说成可续接；这是流式已经向客户端发送事件后的已知边界，需要在文档中写明。
6. 删除旧的 `ResponseSessionInsert` / `save_response_session` / `load_response_session_messages` 公共入口，避免新旧两套状态写入并存；旧列仅由新的保存函数写入派生值。同步把 `tests/identity/storage/sessions.rs`、`tests/identity/cleanup.rs`、`tests/v1/identity_scope.rs` 改为使用 `ResponseState` / `load_response_state`。
7. 增加一个明确的 `store=true` 流式集成测试，证明当前 Codex 的 `store=false` 流量不会误写，而显式存储的流式响应会在完成后写入精确状态。
   测试使用现有的流式原生 Responses mock 变体：断言 mock 收到的上游 `store=false`，客户端请求显式 `store=true`；流结束后按本地 `resp_` ID 读取 `ResponseState`，断言 `request_items` 和 `response_items` 非空，且 `upstream_response_id` 不等于本地 ID。

- [ ] **Step 4: 运行 store 和流式回归**

```text
cargo test --lib responses::tests -- --nocapture
cargo test --lib service::recording::tests::session -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add src/inbound/v1/responses/native.rs src/inbound/v1/responses/chat.rs src/service/recording/state/session.rs src/service/recording/options.rs src/service/activity.rs src/store/storage/mod.rs src/inbound/v1/responses/tests/sessions.rs src/service/recording/tests/session.rs tests/identity/storage/sessions.rs tests/identity/cleanup.rs tests/v1/identity_scope.rs
git commit -m "按 store 语义控制响应状态落库"
```

---

### Task 7: 流式响应 ID 与精确 output items

**Files:**
- Modify: `src/codec/bridge/stream/events.rs`
- Modify: `src/codec/bridge/stream/decode/responses.rs`
- Modify: `src/codec/bridge/stream/decode/responses_tests.rs`
- Modify: `src/codec/bridge/stream/decode/chat_to_responses.rs`
- Modify: `src/codec/bridge/stream/encode/responses.rs`
- Modify: `src/codec/bridge/stream/mod.rs`
- Modify: `src/inbound/v1/responses/native.rs`
- Create: `src/inbound/v1/responses/stream_rewrite.rs`
- Modify: `src/inbound/v1/responses/tests/streaming.rs`

**Interfaces:**
- Produces: `StreamStatsSnapshot.upstream_response_id: Option<String>`。
- Produces: `StreamStatsSnapshot.response_output_items: Vec<Value>`。
- Produces: `rewrite_response_ids(stream, local_id)`：按 SSE 事件缓冲，只改响应 ID。
- `ResponseSessionInsert` 保存 local `response_id` 和 upstream ID，二者不能混用。

- [ ] **Step 1: 写失败测试**

在 `src/inbound/v1/responses/tests/streaming.rs` 增加三个测试：

1. 原生 SSE 跨 chunk：

```text
event: response.created
data: {"type":"response.created","response":{"id":"resp_upstream"}}

event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"item_upstream","type":"function_call"}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_upstream","output":[{"type":"reasoning","id":"rs_1"}]}}
```

拆成多个 `Bytes` 输入，断言：

- 所有 `response.id` 都是本地 `resp_`；
- `item.id` 仍是 `item_upstream`；
- 状态行 `response_items_json` 包含 `reasoning` item；
- 状态行的 `upstream_response_id` 是 `resp_upstream`。

2. 非 JSON data（如 `[DONE]`）原样透传。

3. Chat 桥接的 `response.completed` 包含完整 output，而不是空数组。

- [ ] **Step 2: 运行测试确认失败**

```text
cargo test --lib native_streaming_rewrites_response_ids -- --nocapture
cargo test --lib bridge_stream_completed_includes_output -- --nocapture
```

Expected: FAIL，客户端看到上游 ID 或 completed output 为空。

- [ ] **Step 3: 实现流式改写和捕获**

1. `StreamStatsSnapshot` 把 `response_id` 改名为 `upstream_response_id`。
2. 增加 `response_output_items: Vec<Value>`。因为 `Value` 不实现 `Eq`，从该结构移除 `Eq`，保留 `PartialEq`。
3. `NativeResponsesSseStatsDecoder` 在 `response.completed` 时提取 `pointer("/response/output")`，只有数组才写入 `response_output_items`。
4. `stream_rewrite.rs` 实现有状态 SSE 事件缓冲：
   - 累积完整 `event:` / `data:` 行；
   - 每个 `data:` 行尝试解析 JSON；
   - 只替换 `object["response"]["id"]` 和 `object["response_id"]`；
   - 不改 `object["id"]`、`object["item"]["id"]`、`item_id`、`call_id`；
   - 解析失败的 data 原样输出；
   - `finish()` 刷新最后一个没有空行的事件。
5. `native_responses_sse_with_stats` 返回原始统计流后，用 `rewrite_response_ids(stream, local_id)` 包一层再交给记录链路。统计 decoder 必须读取改写前的上游事件。
6. Chat 桥接的 `responses_completed_sse_with_usage` 接收 output items；`ChatToResponsesSseDecoder` 用已捕获的文本和工具调用构造 `response.output`，确保 completed 事件和落库状态一致。
7. `response_from_stream_session` 优先使用 `snapshot.response_output_items`；如果 `store=true` 且 completed 事件没有完整 output，记录错误并放弃状态写入，不用内部聚合冒充精确 items。

- [ ] **Step 4: 运行流式测试**

```text
cargo test --lib streaming -- --nocapture
cargo test --lib responses::tests::streaming -- --nocapture
```

Expected: PASS；非流式测试不回归。

- [ ] **Step 5: 提交**

```powershell
git add src/codec/bridge/stream/events.rs src/codec/bridge/stream/decode/responses.rs src/codec/bridge/stream/decode/responses_tests.rs src/codec/bridge/stream/decode/chat_to_responses.rs src/codec/bridge/stream/encode/responses.rs src/codec/bridge/stream/mod.rs src/inbound/v1/responses/native.rs src/inbound/v1/responses/stream_rewrite.rs src/inbound/v1/responses/tests/streaming.rs
git commit -m "统一流式响应标识并保存完整输出项"
```

---

### Task 8: TTL 清理与供应商生命周期

**Files:**
- Modify: `src/store/storage/sessions.rs`
- Modify: `src/bootstrap/tasks.rs`
- Modify: `src/bootstrap/serve.rs`

**Interfaces:**
- Produces: `delete_expired_response_states(now)`。
- 启动时执行一次，之后每日执行。
- 删除 provider 不阻断历史 response state 读取；`provider_id` 只作为元数据保留。

- [ ] **Step 1: 写失败测试**

在 `src/store/storage/sessions.rs` 增加：

```rust
#[tokio::test]
async fn deletes_only_expired_response_states() {
    let storage = test_storage().await;
    let (identity_id, provider_id) = seed_identity_and_provider(&storage, "ttl").await;

    identity_response_sessions::ActiveModel {
        response_id: Set("resp_expired".to_string()),
        identity_id: Set(identity_id.clone()),
        previous_response_id: Set(None),
        conversation_id: Set(Some("conv_ttl".to_string())),
        provider_id: Set(provider_id.clone()),
        model: Set("model-ttl".to_string()),
        input_messages_json: Set("[]".to_string()),
        output_items_json: Set("[]".to_string()),
        status: Set("completed".to_string()),
        protocol_format: Set("responses_items_v1".to_string()),
        upstream_response_id: Set(Some("resp_upstream_expired".to_string())),
        request_items_json: Set(Some("[]".to_string())),
        response_items_json: Set(Some("[]".to_string())),
        expires_at: Set(Some("2000-01-01T00:00:00Z".to_string())),
        created_at: Set("2000-01-01T00:00:00Z".to_string()),
    }
    .insert(&storage.db)
    .await
    .expect("insert expired state");

    storage
        .save_response_state(ResponseStateInsert {
            identity_id: &identity_id,
            response_id: "resp_active",
            previous_response_id: None,
            conversation_id: "conv_ttl",
            provider_id: &provider_id,
            model: "model-ttl",
            upstream_response_id: None,
            request_items: &json!([]),
            response_items: &json!([]),
            input_messages: &[],
            response: &response("resp_active", "active"),
        })
        .await
        .expect("save active state");

    identity_response_sessions::ActiveModel {
        response_id: Set("resp_legacy".to_string()),
        identity_id: Set(identity_id.clone()),
        previous_response_id: Set(None),
        conversation_id: Set(None),
        provider_id: Set(provider_id),
        model: Set("model-ttl".to_string()),
        input_messages_json: Set("[]".to_string()),
        output_items_json: Set("[]".to_string()),
        status: Set("legacy".to_string()),
        protocol_format: Set("legacy_internal_v1".to_string()),
        upstream_response_id: Set(None),
        request_items_json: Set(None),
        response_items_json: Set(None),
        expires_at: Set(None),
        created_at: Set("2000-01-01T00:00:00Z".to_string()),
    }
    .insert(&storage.db)
    .await
    .expect("insert legacy state");

    let deleted = storage
        .delete_expired_response_states(Utc::now())
        .await
        .expect("delete expired states");
    assert_eq!(deleted, 1);

    let mut ids = identity_response_sessions::Entity::find()
        .all(&storage.db)
        .await
        .expect("load remaining states")
        .into_iter()
        .map(|row| row.response_id)
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, vec!["resp_active", "resp_legacy"]);
}
```

在 `src/store/storage/sessions.rs` 的测试模块增加第二个测试，复用同一组 helper：

```rust
#[tokio::test]
async fn deleting_a_provider_does_not_block_response_state_history() {
    let storage = test_storage().await;
    let (identity_id, provider_id) = seed_identity_and_provider(&storage, "deleted").await;

    storage
        .save_response_state(ResponseStateInsert {
            identity_id: &identity_id,
            response_id: "resp_history",
            previous_response_id: None,
            conversation_id: "conv_deleted",
            provider_id: &provider_id,
            model: "model-deleted",
            upstream_response_id: None,
            request_items: &json!([]),
            response_items: &json!([]),
            input_messages: &[],
            response: &response("resp_history", "history"),
        })
        .await
        .expect("save state");

    storage
        .delete_provider(&identity_id, &provider_id)
        .await
        .expect("delete provider");

    let state = storage
        .load_response_state(&identity_id, "resp_history")
        .await
        .expect("load state after provider deletion")
        .expect("history remains");
    assert_eq!(state.provider_id, provider_id);
}
```

- [ ] **Step 2: 运行测试确认失败**

```text
cargo test --lib deletes_only_expired_response_states -- --nocapture
cargo test --lib deleting_a_provider_does_not_block_response_state_history -- --nocapture
```

Expected: FAIL，清理任务或外键行为不满足。

- [ ] **Step 3: 实现 TTL 和启动任务**

1. `delete_expired_response_states` 用 `expires_at IS NOT NULL AND expires_at < now` 删除，返回行数。
2. `bootstrap/serve.rs` 在启动时调用一次，失败按现有 cleanup 失败策略处理并记录。
3. `bootstrap/tasks.rs` 增加独立的每日任务，不要和身份清理混在同一个函数里；首次启动清理已经由 `serve.rs` 执行。
4. `provider_id` 外键已在 Task 2 移除；测试只验证行为，不新增外键清理逻辑。

- [ ] **Step 4: 运行存储和启动测试**

```text
cargo test --lib store::storage::sessions::tests:: -- --nocapture
cargo test --lib store::storage::providers::tests:: -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add src/store/storage/sessions.rs src/bootstrap/tasks.rs src/bootstrap/serve.rs
git commit -m "清理过期响应状态并解耦供应商标识"
```

---

### Task 9: 文档、回归和发布门禁

**Files:**
- Modify: `README.md`
- Modify: `docs/architecture/database.md`
- Modify: `docs/superpowers/specs/2026-10-30-responses-state-protocol.md`
- Modify: `docs/reviews/2026-10-30-服务端重构与发布就绪性评审.md`

**Interfaces:**
- Consumes: 前 8 个任务的实现。
- Produces: 文档与代码一致；全量门禁结果明确。

- [ ] **Step 1: 更新文档**

README 写清楚：

- `previous_response_id` 只能使用 Prelay 返回的 `resp_`；
- `store=false` 不保存，不能续接；
- `store=true` 的普通响应保存 30 天；当前 Codex 的默认 `store=false` 流量不会写入状态表；
- `conversation` 当前返回 `unsupported_parameter`；
- 上一轮 `instructions` 不继承；
- 流式响应 ID 使用 Prelay ID，输出项 ID 保持上游值。

数据库文档补充：

- `identity_response_sessions` 精确列和加密列；
- legacy 行不可续接；
- provider 外键移除；
- TTL 清理；
- 现有 `conversation_id` 是内部活动分组，不是协议 conversation。

- [ ] **Step 2: 运行全量验证**

先确认 `TEST_POSTGRES_URL` 已指向专用测试库，然后执行：

```powershell
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo build --release --locked --bin prelay-server
git diff --check
```

Expected: 全部通过。若 `TEST_POSTGRES_URL` 指向的运行中 PostgreSQL 被其他任务占用、Cargo 文件锁未释放或 release 构建被运行中的 `prelay-server.exe` 锁定，保留未验证项并明确记录，不能写成全链路通过。

- [ ] **Step 3: 更新评审报告**

把“`identity_response_sessions` 仍保存完整未脱敏正文”改为“精确 items 已加密保存；legacy 行不可续接”；保留“活动正文由 rollout 文件保存”的边界；记录 `conversation` 仍是显式未实现能力；把旧的 `previous_response_id` 行为标记为已替换。

- [ ] **Step 4: 提交**

```powershell
git add README.md docs/architecture/database.md docs/superpowers/specs/2026-10-30-responses-state-protocol.md docs/reviews/2026-10-30-服务端重构与发布就绪性评审.md
git commit -m "同步 Responses 状态协议文档"
```

---

## 自检

**规范覆盖：**

- `response_id` 本地生成、`previous_response_id`、`store`、`instructions`、30 天 TTL、精确 items、错误体、流式 ID、Conversations 未实现边界均有对应 Task。
- `identity_response_sessions` 的必要性在规范中说明；内部 `conversation_id` 与协议 `conversation` 明确区分。

**类型一致性：**

- `ResponseState`、`ResponseStateInsert`、`save_response_state`、`load_response_state`、`delete_expired_response_states` 在 Task 3 定义，Task 5/6/8 复用。
- `current_input_items`、`continuation_items`、`decode_items_for_chat_bridge`、`encode_native_responses_request_body` 在 Task 5 定义并在后续复用。
- `StreamStatsSnapshot.upstream_response_id` 和 `response_output_items` 在 Task 7 定义，Task 6/7 复用。

**错误契约：**

- 父响应缺失、过期、跨身份、legacy 统一 `400 + previous_response_not_found`。
- `conversation` 未实现统一 `400 + unsupported_parameter`。
- 参数冲突统一 `400 + invalid_request_error`。

**范围边界：**

- 当前部署的 `identity_response_sessions` 为空；没有数据迁移、备份或批量删除任务，表本身保留并升级结构。
- 不实现 Conversations API。
- 不恢复 `/proxy`。
- 不把活动 rollout 当作协议状态源。
- 不推送、不打标签、不部署。
- 真实上游、Docker、GitHub Actions 和部署库不在自动化验证范围内；需要时单独联调。

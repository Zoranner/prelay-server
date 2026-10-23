# 活动 Rollout 文件存储设计

## 背景

当前 `activity_contents` 把一次活动的输入和输出拼成一条规范化文本，并受 `ACTIVITY_CONTENT_MAX_BYTES` 限制。实际数据库中约 11.8 万条正文被标记为截断，工具调用和工具返回也没有完整进入正文。这个模型不能支撑后续的对话审计和团队记忆。

本设计把活动正文改为文件存储，数据库只保存索引和运行元数据。文件是活动正文的唯一来源，不保留 `activity_contents` 的第二份内容。

`identity_response_sessions` 是 Responses 协议续接所需的请求/响应状态缓存，不是活动正文、统计或记忆来源；它可能包含完整历史消息和内联图片，按协议状态单独管理。它不经过活动文件的脱敏，因为续接必须保留原始协议状态；数据库和备份必须按敏感数据管理，且不得通过活动、统计或记忆接口暴露。审计、活动列表和后续记忆只能读取 activity rollout，不得把 response session 当作活动正文。

## 目标

- 每个 activity 一份 append-only JSONL，保存该活动最终可见的请求/响应事件序列；消息和工具调用按最终结果聚合，不是逐 chunk 的 SSE 回放。
- 不按大小截断正文；请求体能被服务接收，正文就写入文件。
- 写入消息、工具调用、工具返回、推理、用量和结束状态。
- PostgreSQL 只保存路径、状态、大小、hash、事件数和 session 关联。
- 不做归档目录、归档迁移或自动清理。
- 保留现有 `identity_activities` 的统计职责和 `memories` / `memory_sources` 的长期记忆职责。

## 非目标

- 不实现团队记忆提取 worker。
- 不把完整正文复制进 PostgreSQL。
- 不保存图片、音频或附件的二进制内容；JSONL 只保存类型、大小和 hash 元数据，当前不提供原始媒体的重建或下载路径。
- 不改变现有 90 天身份清理策略；身份清理同时删除其活动文件。

## 目录与命名

根目录由 `ACTIVITY_ROLLOUT_DIR` 配置，默认 `data/sessions`。

```text
data/
  sessions/
    2026/
      10/
        22/
          rollout-2026-10-22T20-20-10-<activity_id>.jsonl
          rollout-2026-10-22T20-21-05-<activity_id>.jsonl.part
```

- 每个 activity 一个文件，文件名包含 UTC 时间和 `activity_id`。
- 写入期间使用 `.part`，完成后原子重命名为 `.jsonl`。
- 不建 `archived_sessions`。
- 不按 session 追加到同一个文件，避免并发写和事件交错。
- `session_id` 是稳定的会话关联键，只用于数据库聚合，不进入目录层级。Responses 请求从 response session 的 `conversation_id` 继承该值，不能使用每轮变化的 `previous_response_id`。

## 事件格式

每行一个 JSON 对象：

```json
{"seq":1,"timestamp":"2026-10-22T20:20:10.123Z","type":"session_meta","payload":{"activity_id":"...","session_id":"...","identity_id":"...","protocol_in":"responses","protocol_out":"responses","model_requested":"deepseek-flash","model_upstream":"deepseek/deepseek-v4.1-flash","started_at":"..."}}
{"seq":2,"timestamp":"2026-10-22T20:20:10.124Z","type":"message","payload":{"phase":"request","role":"user","content":"..."}}
{"seq":3,"timestamp":"2026-10-22T20:20:10.125Z","type":"tool_call","payload":{"phase":"request","tool_call_id":"call_1","name":"read_file","arguments":"..."}}
{"seq":4,"timestamp":"2026-10-22T20:20:10.126Z","type":"tool_result","payload":{"phase":"request","tool_call_id":"call_1","content":"..."}}
{"seq":5,"timestamp":"2026-10-22T20:20:12.000Z","type":"message","payload":{"phase":"response","role":"assistant","content":"..."}}
{"seq":6,"timestamp":"2026-10-22T20:20:12.001Z","type":"usage","payload":{"input_tokens":123,"output_tokens":45}}
{"seq":7,"timestamp":"2026-10-22T20:20:12.002Z","type":"session_end","payload":{"status":"completed","http_status":200}}
```

事件类型：

- `session_meta`：活动、身份、会话、协议、模型和开始时间。
- `message`：请求或响应中的文本消息。
- `tool_call`：工具名和参数。
- `tool_result`：工具返回内容。
- `reasoning`：上游明确返回的推理文本。
- `usage`：token 用量。
- `error`：错误码、HTTP 状态和错误信息。
- `session_end`：最终状态。

`content` 和 `arguments` 使用原始 JSON 结构，不截断。写入前递归应用现有凭据脱敏，禁止保存 Authorization、Cookie、API Key、Endpoint Token 和设备凭据。

事件是活动终结时生成的内容快照：同一响应文本即使由多个 SSE delta 组成，也只写一条聚合后的 `message`；推理文本同理。文件不承诺支持逐块重放上游流。

## 数据库索引

新增 `activity_rollouts`，不保存正文：

```text
activity_rollouts
  activity_id       VARCHAR PRIMARY KEY
  identity_id       VARCHAR NOT NULL
  session_id        VARCHAR NOT NULL
  rollout_path      VARCHAR NOT NULL UNIQUE
  status            VARCHAR NOT NULL
  event_count       BIGINT NOT NULL DEFAULT 0
  byte_size         BIGINT NOT NULL DEFAULT 0
  sha256            VARCHAR
  created_at        VARCHAR NOT NULL
  completed_at      VARCHAR
  last_error        VARCHAR
```

约束与索引：

- `activity_id` 外键指向 `identity_activities(id)`，删除活动时级联删除索引。
- `rollout_path` 存相对根目录的 POSIX 路径。
- `(identity_id, created_at)` 用于身份范围查询。
- `session_id` 用于同一会话的活动聚合。

`status` 取值：

- `writing`：正在写入。
- `completed`：正常完成并已重命名。
- `failed`：上游、客户端或写入失败。
- `partial`：进程中断或恢复后确认不完整。

## 写入流程

### 非流式

1. 生成 `activity_id` 和 `session_id`。
2. 构建请求事件和响应事件。
3. 写入 `.part` 文件，逐事件 flush。
4. 写入 `session_end`，`sync_all`，重命名为 `.jsonl`。
5. 在数据库事务中插入 `identity_activities` 和 `activity_rollouts`。
6. 文件写入失败时不插入活动记录；数据库失败时文件保留为孤儿文件，启动扫描只告警不删除。

### 流式

1. 在读取首块前创建文件并写入 `session_meta` 和请求事件。
2. 插入 `identity_activities` 和状态为 `writing` 的 `activity_rollouts`。
3. 流式结束时把聚合后的助手消息、工具调用、用量写入文件。
4. 上游错误、客户端断开或空流写入 `error`，再写 `session_end`，状态为 `failed`。
5. 完成后重命名并更新索引；如果磁盘写入失败，保留 `.part` 并记录错误。

### 崩溃恢复

- `.part` 文件不删除。
- 启动时扫描 `ACTIVITY_ROLLOUT_DIR`，对没有索引的孤儿文件只写 warning，不自动删除。
- 对已有索引但状态仍是 `writing` 的记录，先读取已重命名的 JSONL：能确认 `session_end` 为 completed/failed 时恢复两侧状态；文件缺失、末尾不完整或没有 `session_end` 时标记为 `partial`，正文保留。

## 内存与记忆

- `activity_contents`、其状态、租约、重试字段和恢复任务全部移除。
- `memories` 和 `memory_sources` 保留，但本次不实现自动提炼 worker。
- 后续 worker 直接读取 `activity_rollouts.rollout_path` 指向的 JSONL，以 `activity_id + seq` 作为幂等键。
- 记忆来源保存 `activity_id`、`seq` 和证据 hash，不复制整段对话。

## 配置与边界

- `ACTIVITY_ROLLOUT_DIR`：文件根目录，默认 `data/sessions`。
- `PROTOCOL_MAX_REQUEST_BODY_BYTES`：入口限制，默认 256 MiB；本次不调整。
- 不新增正文大小上限。
- 磁盘容量、文件系统单文件大小、备份空间仍是物理限制；磁盘写入失败必须显式标记，不能静默丢弃。

## 兼容与删除

- 现有 `activity_contents` 及其数据直接删除，不做迁移。
- 已发布的单制品扩展包、身份、Provider、Endpoint 和活动统计结构不变。
- 旧 `activity_content_lifecycle_v1` 迁移记录保留在历史中，但新数据库不再创建 `activity_contents`。

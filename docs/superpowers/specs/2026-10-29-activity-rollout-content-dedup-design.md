# 会话正文存储设计

## 目标

按会话保存正文，同一事件在一个会话里只写一次；请求统计留在数据库，activity 不再拥有独立正文文件。

## 会话键

请求已经提供以下 header，按优先级取：

```text
thread-id
session-id
x-session-id
session_id
```

优先使用 `thread-id`，因为它对应实际会话线程；没有时才回退到其他值。只有全部缺失时才使用现有 `conversation_id`。

需要修改 `prelay-server/src/routes/v1/auth.rs` 的 header 提取，并让所有写入链路使用这个会话键，而不是 `context.conversation_id`。

同一 HTTP 请求的候选 failover 和内部重试共享同一个会话键；不能因为换候选或重试而在缺少 header 时生成多个随机会话。

## 上下文窗口

请求还提供：

```text
x-codex-window-id: <thread_id>:<generation>
```

行为已经通过 Codex 源码测试确认：

- 普通对话：generation 为 0
- compact 后：thread_id 不变，generation 加 1
- resume：沿用当前 generation
- fork：thread_id 变化，generation 从 0 开始

因此不能只按 `thread-id` 追加。generation 变化时：

- 在会话文件中写一条 `window_start` 事件
- 不拿新窗口的请求历史去和旧窗口做前缀匹配
- 新窗口事件追加在同一个会话文件里，旧窗口内容保留

activity 数据库同时记录 `thread_id`、`window_id` 和 `generation`。

## 目录

根目录仍是 `ACTIVITY_ROLLOUT_DIR`，默认 `data/sessions`：

```text
data/sessions/
  <identity_id>/
    <thread_id>.jsonl
```

一个会话一个文件，不建 activity 文件，不建 activity `.refs`，不建额外的 identities 顶层目录。

## 文件内容

每个文件按顺序追加该会话的事件，沿用现有 `RolloutEvent` JSONL 格式：

```json
{"seq":1,"timestamp":"...","type":"message","payload":{...}}
```

- 事件正文在每个会话文件中只出现一次。
- 同一会话后续请求携带的历史前缀不重复写入。
- 命令执行、工具调用、工具结果都是会话事件，不是 activity 文件。
- 文件首次创建时写 `session_meta`；初始窗口和 generation 变化时写 `window_start`。
- 文件里不写 activity 级 `session_end`；activity 的完成状态和正文区间由数据库记录。

## 写入

1. 从请求头取 `thread-id`（或回退值），得到会话键。
2. 打开 `data/sessions/<identity_id>/<thread_id>.jsonl`。
3. 如果窗口 generation 未变化，将请求事件与会话文件已有事件按 `(type, payload)` 前缀比较。比较时忽略 `phase`，因为响应事件落盘时是 `response`，下一次请求携带同一段历史时是 `request`。
4. 只追加当前请求中尚未存在的尾部事件。
5. 如果 generation 变化，先追加 `window_start`，再从新窗口的请求事件开始追加，不做旧前缀匹配。
6. 追加响应事件、用量或错误事件，不写 activity 结束事件。
7. 在数据库记录 activity 的会话文件、窗口、起止行号、状态和用量。

同一会话文件的写入锁覆盖整个 activity 生命周期，避免并发追加造成行交错或顺序错乱。

## 数据库

`identity_activities` 继续保存每次请求的统计和状态：

- activity_id
- session_id/thread_id
- window_id/generation
- model/provider/endpoint
- token、延迟、HTTP 状态、错误

`activity_rollouts` 改为保存：

- activity_id
- 会话文件路径
- thread_id、window_id、generation
- start_line、end_line
- activity 区间的事件数、字节数和 hash

不再保存“每个 activity 一个文件”的语义。

## 边界

- 测试阶段旧的活动级正文文件不迁移；数据库只做字段和索引升级，新请求开始使用线程文件。
- 不压缩，不引入 SQLite，不做全文索引。
- activity 只存在于数据库统计和索引中，不参与正文归属。
- 新 thread 自动对应新文件；不需要分支模型。

# Responses 状态协议规范

## 目标

让 `prelay-server` 的 `/v1/responses` 在状态续接上遵循 OpenAI Responses API 的语义，而不是沿用内部消息模型的近似行为。

协议要同时满足：

- `response_id` 由 Prelay 生成，客户端只使用 Prelay ID。
- `previous_response_id`、`store`、`instructions` 的行为与官方一致。
- 续接使用精确的 Responses input/output items，不把内部 `InternalMessage` 当权威正文。
- `store=true` 的普通响应保留 30 天；`store=false` 不落库。
- 未实现的官方能力必须显式拒绝，不能静默忽略或透传。

## 为什么保留状态表

`response_id` 只是客户端使用的句柄，不能替代状态本身。`previous_response_id` 需要按当前身份解析出上一轮精确的 input/output items、完成状态和过期时间；活动 rollout 是审计和展示记录，不是协议状态源。

因此，对严格的 Responses 状态兼容，`identity_response_sessions` 仍然必要；但它要承担的是“ Responses 精确状态存储”，不是“内部消息缓存”。对当前 Codex 这种显式 `store=false` 的无状态调用，它不是必需路径。稳定身份键只解决“属于谁”，不解决“上一轮实际发送和返回了什么”。

## 当前部署数据状态

当前核对的部署库中，`identity_response_sessions` 为空。本次不安排旧行内容迁移、备份或批量删除；只需要完成表结构升级，随后由新协议写入精确状态。不要删除或清空该表。

`legacy` 行处理仍保留在代码里，作为其他环境或异常情况下的防御路径；当前库没有数据，不需要为它安排一次性数据任务。

空表不能直接推断 `/v1/responses` 从未使用：该表只在响应成功且客户端显式或默认启用 `store=true` 时写入。当前 Codex 的 Responses 请求构造会显式发送 `store=false`，因此即使请求已经到达 Prelay，旧表也会保持为空；实际是否使用过应以 `identity_activities` 的 `protocol_in='responses'` 记录为准。

## 官方依据

- [Conversation state](https://developers.openai.com/api/docs/guides/conversation-state)
- [Create a response](https://developers.openai.com/api/reference/resources/responses/methods/create)

已确认的规范事实：

- `previous_response_id` 用于串联响应；不能和 `conversation` 同时使用。
- `conversation` 会把会话 items 前置到本次 `input_items`，本次输入/输出自动加入会话；会话 items 不受 30 天 TTL 限制。
- Response 默认保存 30 天；`store=false` 可以关闭保存。
- 使用 `previous_response_id` 时，上一轮的 `instructions` 不会带到下一轮。
- 无状态 `store=false` 需要客户端回放 output items；必要时包含 `reasoning.encrypted_content`。

## 范围

本规范只覆盖 `/v1/responses` 的响应状态和续接。

Conversations API 是独立子系统，包含 conversation 对象、conversation items 和对应的 CRUD 路由。本阶段不实现它；`conversation` 参数必须返回 `unsupported_parameter`，不能转发给上游或静默忽略。完整 Conversations 兼容由单独规格和计划承接。

## ID 规则

- `response_id`：`resp_<uuid-simple>`，由 Prelay 在请求进入上游前生成，返回给客户端并写入状态表。
- `identity_response_sessions.conversation_id`：现有内部活动分组字段，最终映射为 activity rollout 的 `thread_id`；它不是协议 `conversation`，本阶段不改名、不复用为协议会话 ID。
- 协议 `conversation`：本阶段不实现、不创建；请求中出现时必须显式拒绝。
- `upstream_response_id`：上游返回的原始 ID，只做元数据，不作为客户端句柄。
- `thread_id`、`x-codex-window-id`：活动文件分组，不是协议续接 ID。
- 流式 SSE 事件中的 `response.id` 和顶层 `response_id` 必须改写为 Prelay ID；`item.id`、`item_id`、`call_id` 属于输出项，不能改写。

## 数据模型

`identity_response_sessions` 增加精确状态列：

```text
status                 text not null        -- completed / legacy
protocol_format        text not null        -- responses_items_v1 / legacy_internal_v1
upstream_response_id   text null
request_items_json     text null            -- 加密的精确 Responses input items
response_items_json    text null            -- 加密的精确 Responses output items
expires_at             text null            -- RFC3339；普通响应 created_at + 30 天
```

现有 `input_messages_json`、`output_items_json` 继续作为内部桥接的派生列保留一个迁移周期，不再作为精确续接来源。

约束：

- `(response_id, identity_id)` 仍是联合主键。
- `provider_id` 不做硬外键；供应商删除不能阻断历史状态读取。
- `request_items_json` 保存本轮实际发送的完整 input items，包括从父响应前置的 items。
- `response_items_json` 保存上游返回的完整 output items。
- 新写入使用 `responses_items_v1`；旧行为 `legacy_internal_v1`，不可精确续接。
- 只有 `status='completed'` 且未过期的状态可以作为 `previous_response_id`。

## 请求流程

1. 解析并校验请求；`store` 缺省为 `true`。
2. 校验 `previous_response_id` 与 `conversation` 互斥。
3. `conversation` 非空时返回 `unsupported_parameter`。
4. 有 `previous_response_id` 时，按 `(identity_id, response_id)` 读取状态：
   - 不存在、过期、非 `completed`、legacy 格式：返回稳定错误。
5. 当前 `instructions` 只作用于本次请求，不写入续接 items。
6. 计算本轮 `input_items`：
   - 无父响应：当前输入 items。
   - 有父响应：父 `request_items` + 父 `response_items` + 当前输入 items。
7. 向上游发送请求时移除 `previous_response_id`、`conversation`，并把 `store` 置为 `false`；上游存储语义不影响 Prelay 状态。
8. 只有成功完成的响应才生成可续接状态；`store=true` 写入精确 input/output items，`store=false` 不写入。
9. 返回前把响应 `id` 改写为 Prelay ID；流式路径同样改写 SSE 事件。

`store=false` 的响应仍返回本地 `resp_` ID，但它不能作为后续 `previous_response_id`；再次使用该 ID 时按“父响应不存在”处理。

## 错误契约

`/v1/responses` 使用 OpenAI 风格错误体：

```json
{
  "error": {
    "type": "invalid_request_error",
    "code": "previous_response_not_found",
    "param": "previous_response_id",
    "message": "Previous response not found"
  }
}
```

稳定错误码：

- `invalid_request_error` + `previous_response_id`/`conversation` 互斥：参数冲突，HTTP 400。
- `previous_response_not_found`：父响应不存在、跨身份、已过期、未完成或 legacy 不可续接；HTTP 400。统一返回同一错误，不泄露父响应是否属于其他身份。
- `unsupported_parameter`：`conversation` 尚未实现；HTTP 400。

跨身份查询必须返回“不存在”，不能泄露其他身份是否存在对应响应。

## 迁移边界

- 新 schema 迁移只增列、补索引、移除 provider 外键，不删除旧列。
- 旧行标记为 `legacy_internal_v1`；`previous_response_id` 命中旧行时返回 `previous_response_not_found`，客户端应改用完整历史。
- 启动时对 `legacy_internal_v1` 行的明文列执行一次幂等加密迁移；只改写为 `enc:v1:` 密文，不改变 legacy 标记或续接语义。
- 旧列保留到所有读取方都切到精确 items 后再单独清理。
- 当前库为空，不需要数据回填或删除；如果其他环境存在旧行，它们没有精确 items，也没有可续接语义，TTL 清理只删除 `expires_at` 已过期的精确状态，旧行清理单独评估。

## 流式边界

- 本地响应 ID 在请求进入上游前生成，所有 `response.created`、`response.in_progress`、`response.completed` 等事件中的 `response.id` 都使用该 ID。
- 只改写响应对象的 `id` 和协议定义的顶层 `response_id`；输出项的 `item.id`、事件里的 `item_id`、函数调用的 `call_id` 保持上游值。
- SSE 解析必须按事件缓冲，不能假设一个网络 chunk 对应一个完整 JSON 事件。
- 只有收到 `response.completed` 且能取得完整 `response.output` 时，才允许写入 `response_items_json`；缺失完整输出时记录错误并放弃该次状态写入，不用内部聚合结果冒充精确 items。

## 不变量

- 不把 `thread_id` 当 `previous_response_id`。
- 不把 `InternalMessage`/`InternalOutputItem` 当精确 Responses items。
- 不使用上游 `response.id` 作为客户端句柄。
- 不把客户端的 `store`、`previous_response_id`、`conversation` 原样透传给上游。
- 不为了填充状态表把客户端的 `store=false` 改写为 `true`；是否保存由客户端请求决定。
- 不在 `store=false` 时写入响应状态。
- 不在 `previous_response_id` 缺失或过期时伪造历史。

## 实现落点

- 入口校验、父状态加载和本地 ID 生成：`src/inbound/v1/responses/handler.rs`。
- 精确 items 拼接：`src/inbound/v1/responses/state.rs`。
- 原生与聊天桥接的请求编码和状态保存：`src/inbound/v1/responses/native.rs`、`src/inbound/v1/responses/chat.rs`。
- 精确状态读写、加密和 TTL 清理：`src/store/storage/response_states.rs`。
- 流式响应 ID 改写：`src/inbound/v1/responses/stream_rewrite.rs`。
- 启动清理和每日任务：`src/bootstrap/serve.rs`、`src/bootstrap/tasks.rs`。

回归覆盖父链精确 items、`store=false` 不可续接、跨身份隔离、过期和 legacy 拒绝、流式本地 ID、item/call ID 保持不变、精确输出项保存、供应商删除后历史可读，以及启动清理失败的安全日志。

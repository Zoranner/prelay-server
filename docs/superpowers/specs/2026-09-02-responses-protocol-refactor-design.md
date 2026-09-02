# Responses 协议完整性重构设计

## 目标

修正 Prelay 的 Responses 协议在请求字段保留、usage 统计、流式 SSE、会话续接和错误语义上的数据丢失，使原生 Responses 尽量透传，桥接路径对无法无损转换的能力明确失败。

## 范围

- `POST /v1/responses` 的请求解码、上游请求编码和非流式响应编码。
- Responses、Chat Completions、Anthropic Messages 三种方向的 usage 归一化。
- 转换为 Responses 的流式 SSE 事件。
- `store` 与 `previous_response_id` 在本地桥接会话中的行为。
- 对无法由当前内部模型表达的 Responses 能力增加明确的请求错误。

不包含 Responses WebSocket、后台响应、内置工具执行、图片生成和客户端趋势图改动。

## 设计

### 内部模型

扩展现有 `InternalRequest` 保存 Responses 桥接需要的请求级语义：`instructions`、`store`、`tool_choice`、`parallel_tool_calls`、reasoning 配置、结构化输出配置和可表达的输入项。保留现有三种上游编码器，禁止新增旁路状态模型。

输入内容继续以受控枚举表达。文本、函数调用和函数调用输出可桥接；图片、文件、音频、内置工具和其他无法无损转换的 item 在进入 Chat/Anthropic 桥接前返回 `400`，原生 Responses 上游仍使用原始 JSON 透传。

### Usage

所有 Responses usage 读取统一支持：

- `input_tokens` / `prompt_tokens`
- `output_tokens` / `completion_tokens`
- `output_tokens_details.reasoning_tokens`
- `input_tokens_details.cached_tokens`
- `input_tokens_details.cache_write_tokens`
- 兼容 Anthropic 的 `cache_read_input_tokens` 与 `cache_creation_input_tokens`

对外 Responses usage 输出 `input_tokens_details.cached_tokens` 和 `cache_write_tokens`，并保留 reasoning details。流式 usage 事件只能在收到真实 usage 后更新统计，不用空 usage 覆盖已有值。

### Responses SSE

转换后的事件数据必须是官方 Responses 事件对象，至少包含事件类型和该事件要求的索引、item ID、sequence number 等字段。文本流补齐 response 创建、输出 item/content part、文本增量、文本完成、输出 item 完成和 response completed 事件；completed 事件携带最终 response usage。保留 `data: [DONE]` 作为流结束标记（若上游/客户端路径已有该约定）。

### 会话与持久化

解析 `store`，默认值遵循 Responses API。`store:false` 时不创建本地 response session。非原生桥接的流式响应需要在能够确定完整 response 内容和 ID 后保存 session；若当前流式输出无法提供稳定 response ID，则在请求带 `previous_response_id` 时返回明确的 `400`，不生成不可续接的假状态。

### 错误

保留 HTTP 状态；上游 JSON 错误中的安全字段（错误码和 message）应纳入客户端错误响应和活动摘要，不能把不同性质的上游失败都压成只有状态码的笼统文本。敏感凭据和原始认证头不得透传或记录。

## 不变量

1. 原生 Responses 上游不因桥接内部模型而丢失请求字段。
2. 任何被记录的 cache write/read token 都来自同一份上游 usage，不使用推算值替代。
3. 转换后的 Responses SSE 每个事件的 `data` 都是合法 JSON（`[DONE]` 除外）。
4. `store:false` 不产生可被 `previous_response_id` 查询的本地会话。
5. 不支持的能力必须显式失败，不能静默变成文本或空工具列表。

## 验证

先添加失败测试覆盖官方嵌套 cache usage、Chat 空 choices usage、Responses SSE 事件 schema、请求字段保留、`store:false` 和流式会话边界；再分别验证 Responses 路由、桥接解码器、流统计和全量 Rust 门禁。

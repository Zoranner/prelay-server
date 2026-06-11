# 协议桥接实现验收与差距清单

本文记录当前实现相对 `docs/protocol-bridge-gateway-design.md` 的落地状态。结论基于当前源码结构和现有测试用例观察，不等同于真实客户端联调完成。

## 当前结论

协议桥接主体已经从透明代理演进为多入口、多上游协议的桥接网关。`chat_completions`、`responses`、`anthropic_messages` 和 `ollama_native` 已经进入路由、provider 能力模型、模型目录和统计埋点。

当前可认为后端主链路已完成第一版骨架，但还没有达到完整产品验收。主要缺口集中在三类：

- `responses <-> anthropic_messages` 只支持非流式互转，流式互转明确拒绝。
- 现有验证以 mock upstream 和单元测试为主，缺少 Codex、Claude Code、DeepSeek、OpenAI、Anthropic、Ollama 的真实客户端联调记录。
- 统计已经能记录请求、token、成本估算、延迟、错误和聚合数据，但流式请求统计仍偏粗，`first_token_ms`、流式 token、流式 tool-call 数量和上游 request id 多数没有准确回填。

## 已实现范围

### 用户侧协议入口

已实现的用户侧入口：

- `/v1/chat/completions`：普通 OpenAI Chat Completions 入口。
- `/v1/responses`：OpenAI Responses 入口，面向 Codex 类客户端。
- `/v1/messages`：Anthropic Messages 入口，面向 Claude Code / Anthropic-compatible 客户端。
- `/api/chat`：Ollama Native 入口，主要用于本地模型原生接入。
- `/v1/models` 和 `/models`：模型目录入口，返回 provider、上游协议、上游模型、可用下游协议和能力声明。

### 供给侧协议模型

`ProviderSpec` 已经把上游分为四类：

- `responses`
- `chat_completions`
- `anthropic_messages`
- `ollama_native`

当前 provider 类型映射大致为：

- `openai` 映射为原生 Responses 上游。
- `openai_compatible` 映射为 Chat Completions 上游。
- `anthropic` 和 `anthropic_compatible` 映射为 Anthropic Messages 上游。
- `ollama_native` 映射为 Ollama Native 上游。
- 未识别 provider 默认按 Chat Completions 处理，但高级能力默认关闭。
- provider 能力模型已覆盖工具调用、reasoning、tool choice、并行工具调用、system message、结构化输出、流式 usage，以及上下文和输出 token 上限。
- provider 配置支持通过 `capabilities` 覆盖默认能力声明，旧配置没有覆盖字段时继续按 provider 类型推导默认能力。

### 已实现的协议方向

当前已落地的方向：

- `chat_completions -> chat_completions`
- `chat_completions -> responses`
- `chat_completions -> anthropic_messages`
- `responses -> responses`
- `responses -> anthropic_messages`，仅非流式。
- `anthropic_messages -> anthropic_messages`
- `anthropic_messages -> responses`，仅非流式。
- `ollama_native -> ollama_native`
- `ollama_native -> chat_completions`
- `ollama_native -> responses`
- `ollama_native -> anthropic_messages`

`ollama_native -> chat_completions` 已作为 OpenAI Chat Completions 入口的上游路径实现，普通 OpenAI-compatible 客户端可以通过 `/v1/chat/completions` 调用 Ollama Native 上游。

### 流式桥接

已实现的流式路径：

- Chat Completions 上游流式透传到 `/v1/chat/completions`。
- Chat Completions 上游流式转换为 Responses SSE。
- Chat Completions 上游流式转换为 Anthropic Messages SSE。
- 原生 Responses 上游流式直通到 Responses 入口。
- 原生 Anthropic Messages 上游流式直通到 Messages 入口。
- Ollama Native 流式转换为 Responses SSE。
- Ollama Native 流式转换为 Chat Completions SSE。
- Ollama Native 流式转换为 Anthropic Messages SSE。
- Ollama Native 流式原生透传到 `/api/chat`。

当前明确未实现的流式路径：

- `responses -> anthropic_messages` 流式互转。
- `anthropic_messages -> responses` 流式互转。

这两类请求当前会返回失败，并写入 `request_logs`，错误码为 `unsupported_stream_bridge`。

### 会话和工具调用

已实现内容：

- Responses 请求支持 `previous_response_id` 本地会话链。
- 非流式 Responses 响应会保存会话，用于后续工具回合恢复上下文。
- Chat Completions 工具调用可以编码到 Responses 输出。
- Anthropic `tool_use` / `tool_result` 与内部工具模型已有基础互映。
- 对不支持工具调用的 provider，会通过兼容性检查拒绝请求并记录统计。

仍需补齐的内容：

- DeepSeek reasoning / thinking 回放只具备设计预期，当前没有看到完整 provider 特化处理闭环。
- 流式 tool-call 的统计和跨协议状态机还不完整。
- Responses 会话链主要服务 Responses 入口，Anthropic Messages 侧没有等价的持久会话链能力。

## 统计观测状态

### 已实现能力

数据库中已有 `request_logs` 表，字段覆盖：

- 入站协议、出站协议、上游协议。
- provider 和模型。
- 成功或失败状态。
- HTTP 状态。
- 错误码和错误信息。
- 是否流式。
- 输入、输出和 reasoning token。
- 成本估算和币种。
- 总耗时、上游耗时、首 token 耗时。
- 工具调用数量。
- 上游 request id。
- 元数据 JSON。

后端已提供统计 API：

- `GET /api/stats/overview`
- `GET /api/stats/requests`
- `GET /api/stats/models`
- `GET /api/stats/providers`

前端已有统计视图，覆盖：

- 总请求、成功、失败、输入 token、输出 token。
- 最近请求列表。
- 请求状态、provider、模型、协议、HTTP 状态、上游 ID、错误、token、耗时。
- 模型聚合。
- Provider 聚合。

### 统计边界

当前统计是观测能力，不是控制能力。代码和设计均未引入：

- 限额。
- 预算。
- 扣费。
- 充值余额。
- 自动限流。
- 团队额度。
- 账单结算。

默认统计也没有保存完整 prompt 和完整响应正文，这符合当前隐私边界。

### 统计差距

当前统计仍有明显边界：

- 流式请求通常在返回流之前先写成功日志，后续流是否中断、最终 token、最终 tool-call 数量不能准确回填。
- `first_token_ms` 字段已建模和聚合展示，但多数真实请求路径写入 `None`。
- `upstream_request_id` 字段已建模和展示，但多数 provider 响应路径没有从响应头或响应体提取。
- 上游错误多数只记录 HTTP 状态，没有统一解析 provider error code、error message 和 request id。
- 原生 Responses / Anthropic Messages 流式直通时，usage 事件没有被解析并归一化到统计字段。
- 成本估算依赖本地价格配置，无法匹配价格时只能保持空值；这不是阻塞问题，但需要在产品界面明确显示。

## 验收状态

### 已具备本地验收基础

现有测试和代码已经覆盖以下基础行为：

- 协议入口鉴权。
- 模型别名解析。
- 模型别名按下游协议过滤。
- Chat Completions 非流式和流式代理。
- Responses 到 Chat Completions 上游。
- Responses 到原生 Responses 上游。
- Responses 到 Ollama Native 上游。
- Responses 到 Anthropic Messages 上游的非流式桥接。
- Anthropic Messages 到 Chat Completions 上游。
- Anthropic Messages 到原生 Anthropic Messages 上游。
- Anthropic Messages 到 Responses 上游的非流式桥接。
- Anthropic Messages 到 Ollama Native 上游。
- Ollama Native 原生 `/api/chat`。
- Chat Completions 到 Ollama Native 上游。
- `previous_response_id` 多跳历史。
- function tool call 回合。
- 统计 API 聚合。

这些验收依赖本地 mock upstream，能证明内部转换和路由分支基本成立，但不能证明真实客户端兼容性。

### 尚未完成的真实联调

需要补充真实联调记录：

- Codex 使用 `wire_api = "responses"` 通过本服务调用 DeepSeek Chat Completions 上游。
- Codex 通过本服务调用 OpenAI 原生 Responses 上游。
- Codex 通过本服务调用 Ollama Native 上游。
- Claude Code / Anthropic-compatible 客户端通过本服务调用 DeepSeek Chat Completions 上游。
- Claude Code / Anthropic-compatible 客户端通过本服务调用 Anthropic 原生 Messages 上游。
- Claude Code / Anthropic-compatible 客户端通过本服务调用 OpenAI Responses 上游的非流式桥接。
- 普通 OpenAI-compatible 客户端通过 `/v1/chat/completions` 调用 Chat Completions 上游。
- Ollama 客户端通过 `/api/chat` 调用 Ollama Native 上游。

真实联调需要至少记录：

- 客户端配置。
- provider 配置。
- 请求是否流式。
- 是否包含工具调用。
- 响应是否能被客户端正确消费。
- `request_logs` 是否记录正确的协议、provider、模型、状态、token 和错误信息。

## 与设计文档的差距

### 协议方向差距

设计要求的多数方向已实现，但仍有缺口：

- `responses -> anthropic_messages` 仅非流式可用。
- `anthropic_messages -> responses` 仅非流式可用。

设计中禁止的方向仍保持关闭：

- 入站 `responses` 没有被降格输出到 Chat Completions。
- 入站 `anthropic_messages` 没有被降格输出到 Chat Completions。

### 流式状态机差距

当前已有多条流式文本路径，但跨 agent 协议互转仍没有状态机：

- Responses SSE 到 Anthropic Messages SSE 尚未实现。
- Anthropic Messages SSE 到 Responses SSE 尚未实现。
- 跨协议流式 tool-call 增量没有完整状态聚合。
- 流式 usage 事件没有统一解析。

这部分不应简单透传，需要明确事件状态机，否则容易造成客户端等待、工具调用不闭合或统计不准确。

### Provider 归一化差距

当前 provider 能力模型已经从 `tool_calls` 扩展为能力画像：

- 是否支持工具调用。
- 是否支持 reasoning / thinking。
- 是否支持 tool choice。
- 是否支持 parallel tool calls。
- 是否支持 system message。
- 是否支持 JSON schema / structured output。
- 是否支持流式 usage。
- 最大上下文和最大输出 token。

当前已经参与强制拒绝判断的字段包括工具调用、reasoning、tool choice、并行工具调用、system message、结构化输出、流式 usage 和最大输出 token。仍需继续细化的是能力使用策略：哪些字段可以安全忽略、哪些需要降级、哪些必须拒绝，以及不同 provider 的真实能力默认值是否准确。

### 错误和审计差距

当前错误处理能落库失败状态，但审计质量还不够：

- 上游错误体没有统一结构化。
- 上游 request id 没有普遍提取。
- 兼容性拒绝和上游失败的错误分类还不够细。
- 流式中途失败没有最终落库状态修正。

## 建议优先级

### 先收口真实客户端验收

先补真实联调，而不是继续扩展协议面。当前最能暴露问题的是：

- Codex + DeepSeek Chat Completions，上游走 `/v1/responses`。
- Claude Code + DeepSeek Chat Completions，上游走 `/v1/messages`。
- Codex + OpenAI Responses，原生上游直通。
- Claude Code + Anthropic Messages，原生上游直通。
- Codex / Claude Code + Ollama Native，确认本地模型降级边界。

### 再补统计准确性

统计应先补流式请求的准确性：

- 记录首 token 时间。
- 尽量从最终 usage 或 done 事件回填 token。
- 提取上游 request id。
- 流式中途失败时记录失败或部分完成状态。
- 明确无法统计的字段为空，而不是估算成错误值。

### 最后推进跨 agent 流式互转

`responses <-> anthropic_messages` 流式互转应放在真实客户端验收和统计收口之后。它需要独立状态机，不适合用简单字节透传实现。

首个可做方向建议是：

- Anthropic Messages SSE 到 Responses SSE。

原因是 Claude Code 侧工具调用语义较明确，落到 Responses 输出时更容易建立 `response.output_item.*` 和 `response.function_call_arguments.*` 的事件序列。

## 当前可发布边界

如果按内部试用口径，当前可以作为协议桥接网关的早期版本使用：

- 支持 Codex 通过 Responses 调用 Chat Completions、Responses 和 Ollama 上游。
- 支持 Claude Code 类客户端通过 Messages 调用 Chat Completions、Anthropic Messages 和 Ollama 上游。
- 支持普通 OpenAI-compatible 客户端调用 Chat Completions 上游。
- 支持基础统计页面和请求日志。

如果按对外产品口径，仍不应宣称完整支持：

- 不应宣称完整支持 Responses 与 Anthropic Messages 流式互转。
- 不应宣称已完成 Codex / Claude Code 真实兼容性认证。
- 不应宣称统计 token、成本、首 token、上游 request id 在所有协议上都准确。
- 不应宣称支持限额、预算、计费或企业审计。

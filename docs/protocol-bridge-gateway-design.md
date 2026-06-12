# provider-relay 协议桥接网关设计

## 背景

`provider-relay` 当前是一个轻量的上游密钥管理和透明代理服务：管理页创建 provider 配置，后端按 token 查到上游 `base_url` 和 `api_key`，再把请求转发到上游接口。

后续目标不是继续扩展透明代理，而是把它做成面向 AI 编程工具的协议桥接网关。用户侧主要使用三类协议：

- OpenAI Responses API：面向 Codex。
- OpenAI Chat Completions API：面向普通 OpenAI-compatible 客户端。
- Anthropic Messages API：面向 Claude Code / Anthropic-compatible 客户端。

供给侧需要覆盖三类上游：

- Chat Completions：DeepSeek、智谱、MiniMax、Kimi、OpenAI-compatible 计费接口等。
- OpenAI Responses API：OpenAI 原生 Responses 端点，以及支持 Responses 的 Codex 类计费接口。
- Anthropic Messages API：Anthropic 原生 Messages 端点，以及 Claude Code / Anthropic-compatible 计费接口。

设计目标是把主流计费接口包装成 Codex、Claude Code 和普通客户端可用的统一服务，同时保留清晰的协议边界和统计观测能力。

## 参考项目结论

`.refs` 下当前有五个参考项目：

- `codex-bridge`：最直接参考。它专注于 Codex Responses API 与 Chat Completions 的桥接，覆盖流式 SSE、tool calls、`previous_response_id`、DeepSeek thinking/reasoning 回放、入站鉴权和模型目录。
- `GodeX`：适合作为结构参考。它使用 `ProviderSpec`、能力声明、兼容性规划和 Responses 到 Chat Completions 的桥接内核，适合借鉴边界设计。
- `litellm`：适合作为网关形态参考。它把 `/v1/chat/completions`、`/v1/responses`、`/v1/messages` 做成独立入口，并把 provider transformation 放到独立模块。
- `aura-llm-gateway`：适合作为生产化参考。它包含 provider、router、metrics、cost tracking、cache、multi-tenancy 等完整网关能力，但当前项目不应直接照搬其复杂度。
- `cc-switch`：适合作为客户端生态参考。它说明用户会同时管理 Claude Code、Codex、Gemini CLI 等工具，也说明 `chat_completions`、`anthropic_messages`、`codex_responses` 这类协议分类是实际产品概念。

本项目应借鉴 `codex-bridge` 的桥接细节、`GodeX` 的 provider 规约、`litellm` 的入口分层，以及 `cc-switch` 的客户端接入形态，但保持 Rust 服务的小型化实现。

## 协议范围

### 支持的用户侧协议

- `responses`：OpenAI Responses API，对外暴露 `/v1/responses`，主要服务 Codex。
- `chat_completions`：OpenAI Chat Completions API，对外暴露 `/v1/chat/completions`，服务普通 OpenAI-compatible 客户端。
- `anthropic_messages`：Anthropic Messages API，对外暴露 `/v1/messages`，服务 Claude Code / Anthropic-compatible 客户端。

### 支持的供给侧协议

- `chat_completions`：主力上游协议。
- `responses`：OpenAI Responses 原生上游协议。
- `anthropic_messages`：Anthropic Messages 原生上游协议。

### 允许的转换方向

```text
chat_completions -> chat_completions
chat_completions -> responses
chat_completions -> anthropic_messages

responses -> responses
responses -> anthropic_messages

anthropic_messages -> anthropic_messages
anthropic_messages -> responses
```

其中 `responses -> anthropic_messages` 和 `anthropic_messages -> responses` 是面向 Codex / Claude Code 互通的高级桥接，优先级低于 `chat_completions -> responses` 和 `chat_completions -> anthropic_messages`。

### 禁止的转换方向

```text
responses -> chat_completions
anthropic_messages -> chat_completions
```

原因：

- Responses 和 Anthropic Messages 通常承载 Codex / Claude Code 这类 agent 客户端语义。
- 将入站 agent 协议降格为普通 Chat Completions 出口容易造成能力错配、审计困难和滥用判定风险。
- 本项目的定位是把普通上游和本地模型升格为 AI 编程工具可用协议，而不是把高级 agent 协议包装成普通计费接口。
- 该限制不影响将 Responses 或 Anthropic Messages 作为上游原生协议使用；原生上游可以直接服务同协议用户侧入口，也可以在后期桥接到另一种 agent 协议。

## 产品边界

### 做

- 多协议入口：`/v1/chat/completions`、`/v1/responses`、`/v1/messages`。
- 模型列表：`/v1/models`。
- provider 配置和模型别名管理。
- Chat Completions 上游适配。
- Responses 原生上游适配。
- Anthropic Messages 原生上游适配。
- Responses 桥接，支持 Codex 使用。
- Anthropic Messages 桥接，支持 Claude Code 使用。
- 流式 SSE 桥接。
- function tool calls 桥接。
- `previous_response_id` 本地会话链。
- reasoning / thinking 能力映射和降级。
- 统计观测：请求、token、成本估算、错误、延迟、工具调用。

### 不做

- 限额。
- 预算。
- 扣费。
- 充值余额。
- 自动限流。
- 团队额度。
- 账单结算。
- 默认保存完整 prompt 和响应正文。
- 复杂组织、多租户和企业审计。
- Gemini Native 和 Bedrock Converse 首期支持。

## 总体架构

整体分为五层：

```text
HTTP Routes
  -> Downstream Decoders
  -> Internal Bridge Model
  -> Upstream Adapters
  -> Downstream Encoders
```

### HTTP Routes

负责协议入口、鉴权、请求生命周期计时和错误响应。

建议模块：

```text
src/routes/chat.rs
src/routes/responses.rs
src/routes/messages.rs
src/routes/models.rs
src/routes/proxy.rs
```

`routes/proxy.rs` 保留现有透明代理能力，不与桥接入口混合。

### Downstream Decoders

把用户侧协议转换为内部请求：

```text
src/bridge/chat_decode.rs
src/bridge/responses_decode.rs
src/bridge/anthropic_decode.rs
```

首期只需要 `responses_decode` 和 `chat_decode`。`anthropic_decode` 在支持 `/v1/messages` 时补齐。

### Internal Bridge Model

内部模型承载协议无关语义：

```text
src/bridge/internal.rs
src/bridge/tools.rs
src/bridge/sessions.rs
src/bridge/stream.rs
```

核心对象：

- `InternalRequest`
- `InternalMessage`
- `InternalContentPart`
- `InternalTool`
- `InternalToolChoice`
- `InternalResponse`
- `InternalOutputItem`
- `InternalUsage`
- `InternalStreamDelta`
- `CompatibilityDiagnostic`

内部模型是协议转换的中心。新增协议时应新增 decoder / encoder / adapter，不应在路由里堆叠分支。

### Upstream Adapters

把内部请求发送到上游：

```text
src/providers/spec.rs
src/providers/chat_completions.rs
src/providers/responses.rs
src/providers/anthropic_messages.rs
```

`ProviderSpec` 描述 provider 能力：

- provider id 和名称。
- 上游协议。
- 默认 base URL。
- auth scheme。
- 支持模型。
- 工具调用能力。
- reasoning / thinking 能力。
- tool choice 能力。
- parallel tool calls 能力。
- system message 能力。
- JSON schema / structured output 能力。
- stream usage 能力。
- 最大上下文和最大输出 token。
- usage 字段映射。

### Downstream Encoders

把内部响应编码为用户侧协议：

```text
src/bridge/chat_encode.rs
src/bridge/responses_encode.rs
src/bridge/anthropic_encode.rs
```

首期优先实现 `responses_encode` 和 `chat_encode`。`anthropic_encode` 在支持 `/v1/messages` 时实现。

## 请求流程

### Responses 入口调用 Chat Completions 上游

```text
Codex
  POST /v1/responses
    -> 入站鉴权
    -> 解析 model / alias
    -> responses_decode
    -> session chain 合并
    -> compatibility diagnostics
    -> chat_completions adapter
    -> responses_encode
    -> 写 request_logs
```

流式请求：

```text
Codex
  POST /v1/responses stream=true
    -> 上游 Chat Completions SSE
    -> InternalStreamDelta
    -> Responses SSE events
    -> 持续统计 usage / tool calls / first_token_ms
    -> 结束后写 request_logs
```

### Chat Completions 入口调用上游

```text
OpenAI-compatible client
  POST /v1/chat/completions
    -> 入站鉴权
    -> chat_decode
    -> 选择 chat_completions 上游
    -> adapter 调用
    -> chat_encode
    -> 写 request_logs
```

当上游也是 Chat Completions 时，可以进行近似透传，但仍应经过统一的鉴权、模型解析、统计和错误处理。

### Anthropic Messages 入口调用 Chat Completions 上游

第二阶段实现：

```text
Claude Code
  POST /v1/messages
    -> 入站鉴权
    -> anthropic_decode
    -> 选择 chat_completions 上游
    -> adapter 调用
    -> anthropic_encode
    -> 写 request_logs
```

重点是将 `tool_use` / `tool_result` 与 Chat Completions 的 `tool_calls` / `role=tool` 正确互映。

## 会话和工具调用

Codex 和 Claude Code 的工具调用不是普通单轮聊天。必须支持会话链和工具回合。

### Responses 会话链

`previous_response_id` 由网关本地维护。每次 Responses 请求结束后，存储：

- request input items。
- response output items。
- previous response id。
- provider。
- model。
- reasoning content 摘要或原始 provider 所需字段。
- 创建时间。

后续请求带 `previous_response_id` 时，网关重建历史上下文并前置到当前输入。

### DeepSeek thinking 安全处理

参考 `codex-bridge` 的经验，DeepSeek thinking 模式与工具调用回合存在额外要求：上游可能要求历史 assistant tool call 附带 `reasoning_content`。Codex 不会天然替网关保存并回放这些字段。

策略：

- 捕获上游返回的 `reasoning_content`。
- 与 response id / tool call id 关联保存。
- 后续工具回合能回放时回放。
- 无法安全回放时，自动关闭 thinking 或降级 reasoning，并记录兼容性诊断。

### 工具调用循环保护

统计连续 tool-call-only 响应次数，用于观测和调试。首期只记录，不做强制限流。必要时可以在响应中注入兼容性诊断，但不做预算或限额管控。

## 统计观测

统计是产品内建能力，但只做观测，不做管控。

### 请求日志

新增 `request_logs` 表：

```text
id
created_at
protocol_in
protocol_out
protocol_upstream
provider_id
provider_name
model_requested
model_upstream
proxy_token_id
status
http_status
error_code
error_message
is_streaming
input_tokens
output_tokens
reasoning_tokens
cache_read_tokens
cache_write_tokens
estimated_cost
currency
latency_ms
upstream_latency_ms
first_token_ms
tool_call_count
upstream_request_id
metadata_json
```

默认不保存完整请求体和响应体。`error_message` 应脱敏，不能包含 API key、代理 token 或完整 prompt。

### 模型价格

新增 `model_prices` 或配置文件：

```text
provider
model
input_price_per_1m
output_price_per_1m
cache_read_price_per_1m
cache_write_price_per_1m
reasoning_price_per_1m
currency
updated_at
```

价格可以先用本地 JSON/TOML 配置，不做在线同步。无法匹配价格时，`estimated_cost` 为空，并在管理页显示 `unknown`。

### 统计页面

管理页增加统计视图：

- 总览：今日请求数、token、估算成本、成功率、平均延迟。
- 明细：请求列表，支持按时间、provider、model、协议、状态筛选。
- 模型统计：按模型聚合请求数、token、成本、错误率、平均耗时。
- Provider 统计：按上游 provider 聚合成功率、错误率、平均延迟、首 token 时间。

### 统计 API

建议提供：

```text
GET /api/stats/overview
GET /api/stats/requests
GET /api/stats/models
GET /api/stats/providers
```

这些接口只读，不参与限额和预算控制。

## 配置模型

现有 `provider_configs` 只适合透明代理，需要扩展。

建议新增或迁移为：

```text
providers
  id
  name
  upstream_protocol
  base_url
  api_key
  auth_scheme
  enabled
  created_at
  updated_at

provider_models
  id
  provider_id
  model
  display_name
  enabled
  capabilities_json

provider_configs
  capabilities_json

model_aliases
  id
  alias
  provider_id
  upstream_model
  downstream_protocols_json
  enabled
```

首期可以先用兼容现有表的轻量扩展，不必一次性迁移成完整模型。设计上应避免继续把 provider 类型写死在前端枚举中。

## 鉴权

现有代理 token 可以继续作为入站 key。后续需要区分：

- 管理 API 鉴权。
- 用户侧协议入口鉴权。
- 上游 provider API key。

首期协议入口支持：

```text
Authorization: Bearer <proxy token>
x-api-key: <proxy token>
```

管理 API 当前没有独立鉴权，作为后续安全任务处理。协议桥接上线前，如果服务可能暴露到非本机网络，必须先补管理鉴权。

## 实施阶段

### Chat Completions 到 Responses

目标：DeepSeek Chat Completions 上游可被 Codex 通过 Responses API 使用。

范围：

- `/v1/models`
- `/v1/responses`
- DeepSeek / OpenAI-compatible Chat Completions adapter
- 非流式 Responses 输出
- 流式 Responses SSE 输出
- function tool calls
- `previous_response_id`
- reasoning effort 映射
- DeepSeek thinking 安全处理
- `request_logs` 基础埋点

验收：

- Codex 配置 `wire_api = "responses"` 后能通过本服务调用 DeepSeek。
- 普通文本请求可用。
- 流式输出可用。
- 至少 function tool call 回合不崩。
- 请求日志能记录请求数、模型、provider、token、延迟、状态。

### 统计页面

目标：形成完整观测闭环。

范围：

- 统计 API。
- 管理页统计总览。
- 请求明细。
- 模型/provider 聚合。
- 价格表配置和成本估算。

验收：

- 能按日查看请求数、token、成本、成功率、延迟。
- 能定位失败请求的 provider、模型、协议和错误类型。
- 无价格配置的模型不会阻断请求。

### 原生 Responses 与 Anthropic Messages 上游接入

目标：支持供给侧直接接入 OpenAI Responses 和 Anthropic Messages 原生协议。

范围：

- `src/providers/responses.rs`。
- `src/providers/anthropic_messages.rs`。
- Responses 上游直通服务 `/v1/responses`。
- Anthropic Messages 上游直通服务 `/v1/messages`。
- 原生上游返回的 usage、stream event、request id、错误结构归一化到统计模型。
- 支持同协议直通和后续 agent 协议互转的内部响应模型。

验收：

- OpenAI Responses 原生上游可被 Codex 通过本服务调用。
- Anthropic Messages 原生上游可被 Claude Code / Anthropic-compatible 客户端通过本服务调用。
- 原生上游请求能写入统计日志。
- 原生上游不被降格转成 Chat Completions 出口。

### Chat Completions 到 Anthropic Messages

目标：Chat Completions 上游可被 Claude Code / Anthropic-compatible 客户端使用。

范围：

- `/v1/messages`
- Anthropic Messages decoder / encoder
- Chat tool calls 到 `tool_use`
- `tool_result` 到 Chat `role=tool`
- Anthropic SSE 事件输出
- 统计复用。

验收：

- Claude Code 类客户端能通过本服务调用 DeepSeek / OpenAI-compatible 上游。
- 工具调用结构正确，不降级成纯文本。

### Responses 与 Anthropic Messages 互转

目标：仅在明确需求出现时支持 `responses <-> anthropic_messages`。

范围：

- Responses decoder 到 Internal。
- Anthropic decoder 到 Internal。
- 两端 encoder 复用。
- 严格禁止输出到 Chat Completions。

验收：

- Codex 协议与 Claude Code 协议可互相复用部分 agent 语义。
- 对无法映射的字段产生兼容性诊断。

## 风险和约束

### 工具调用语义不一致

Responses、Chat Completions、Anthropic Messages 对工具调用的表达不同。必须使用内部工具模型统一表达，不能在字符串层面拼接。

### 流式事件复杂

Responses SSE 和 Anthropic SSE 都有状态机语义。流式桥接应由 `InternalStreamDelta` 和状态机驱动，避免手工拼接零散事件导致客户端卡死。

### Reasoning 字段差异

不同 provider 对 reasoning / thinking 支持不同。必须通过 compatibility diagnostics 做映射、默认值补齐、降级或透传决策，并把无法完整表达的字段记录到诊断信息。

### 会话链膨胀

`previous_response_id` 重建历史会导致上下文增长。首期采用 LRU + TTL + 简单截断策略，后续再考虑摘要或持久化策略。

### 隐私和数据膨胀

统计默认只保存摘要，不保存完整 prompt 和响应。调试 payload capture 如果需要，必须单独开关、限时、脱敏。

### 参考项目复杂度

LiteLLM 和 Aura 的企业能力不应直接带入。当前项目优先保持小型、可理解、可本地部署。

## 首期文件规划

阶段一预计新增或修改：

```text
src/routes/models.rs
src/routes/responses.rs
src/bridge/internal.rs
src/bridge/responses_decode.rs
src/bridge/responses_encode.rs
src/bridge/sessions.rs
src/bridge/stream.rs
src/providers/mod.rs
src/providers/spec.rs
src/providers/chat_completions.rs
src/stats.rs
src/db.rs
src/models.rs
src/main.rs
```

前端统计页面在阶段二处理：

```text
web/src/views/StatsView.vue
web/src/api/stats.ts
web/src/components/stats/*
```

## 验证策略

Rust 代码修改后执行：

```powershell
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

Node.js / 前端命令使用 bun：

```powershell
bun run build
bun run lint
```

协议桥接需要补充集成测试或 smoke 测试：

- `/v1/models` 返回模型列表。
- `/v1/responses` 字符串输入。
- `/v1/responses` message array 输入。
- `/v1/responses` 非流式 tool call。
- `/v1/responses` 流式文本。
- `/v1/responses` 流式 tool call。
- `previous_response_id` 工具回合。
- DeepSeek thinking 降级。
- request log 写入。

测试中的上游应使用本地 mock server，不依赖真实 API key。

## 结论

`provider-relay` 应演进为小型协议桥接网关：用户侧支持 Chat Completions、Responses、Anthropic Messages，供给侧支持 Chat Completions、Responses、Anthropic Messages。核心转换通过内部模型承载，不做任意协议互转，明确禁止将入站 `responses` 和 `anthropic_messages` 降格输出到 `chat_completions`。

首期先做 `chat_completions -> responses`，以 DeepSeek 让 Codex 可用为验收目标，并同时埋入统计观测字段。统计只做观测，不做限额、预算和扣费。后续再补 Anthropic Messages 和管理页统计。

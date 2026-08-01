# provider-relay 协议桥接网关设计

## 背景

`provider-relay` 定位为面向 AI 编程工具的小型协议桥接网关。管理端维护 Provider、Provider 模型、Interface 及 Interface 模型映射；协议入口使用 Interface token 确定可访问的客户端模型，再解析到上游 Provider 和模型。

用户侧支持三类协议：

- OpenAI Responses API：面向 Codex。
- OpenAI Chat Completions API：面向普通 OpenAI-compatible 客户端。
- Anthropic Messages API：面向 Claude Code / Anthropic-compatible 客户端。

供给侧需要覆盖三类上游：

- Chat Completions：DeepSeek、智谱、MiniMax、Kimi、OpenAI-compatible 计费接口等。
- OpenAI Responses API：OpenAI 原生 Responses 端点，以及支持 Responses 的 Codex 类计费接口。
- Anthropic Messages API：Anthropic 原生 Messages 端点，以及 Claude Code / Anthropic-compatible 计费接口。

设计目标是把主流计费接口包装成 Codex、Claude Code 和普通客户端可用的统一服务，同时保留清晰的协议边界和统计观测能力。

## 参考项目结论

仓库 `.refs` 包含五个参考项目：

- `codex-bridge`：最直接参考。它专注于 Codex Responses API 与 Chat Completions 的桥接，覆盖流式 SSE、tool calls、`previous_response_id`、DeepSeek thinking/reasoning 回放、入站鉴权和模型目录。
- `GodeX`：适合作为结构参考。它使用 `ProviderSpec`、能力声明、兼容性规划和 Responses 到 Chat Completions 的桥接内核，适合借鉴边界设计。
- `litellm`：适合作为网关形态参考。它把 `/v1/chat/completions`、`/v1/responses`、`/v1/messages` 做成独立入口，并把 provider transformation 放到独立模块。
- `aura-llm-gateway`：适合作为生产化参考。它包含 provider、router、metrics、cost tracking、cache、multi-tenancy 等完整网关能力，但本项目不应直接照搬其复杂度。
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
- Gemini Native 和 Bedrock Converse。

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

每种用户侧协议使用独立 decoder。路由只负责鉴权、生命周期和错误响应，不承担协议字段转换。

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

每种用户侧协议使用独立 encoder。encoder 只消费内部模型，不直接依赖上游响应结构。

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

统计连续 tool-call-only 响应次数，用于观测和调试。该指标不触发强制限流；必要时可以在响应中注入兼容性诊断，但不做预算或限额管控。

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

配置关系以 Provider 和 Interface 为中心：

```text
provider_configs
  -> provider_models

interfaces
  -> interface_models
       -> provider_configs + provider_models
```

Provider 保存请求中的 `models` 表示保存后的完整模型集合，不是增量命令。创建请求必须携带该集合；更新请求传入 `models` 时替换完整集合，缺省时保留原集合以兼容已有调用方。服务端统一去除模型名首尾空白，并拒绝空名称和规范化后的重复名称。

Interface 保存请求中的 `models` 表示保存后的完整映射集合。每项包含 `provider_id`、`upstream_model` 和可选 `model_name`；`model_name` 为空时使用 `upstream_model`。同一 Interface 内的客户端模型名必须唯一，且每项都必须引用已存在的 Provider 模型。

Provider 配置及完整模型集合在同一 SQLite 事务中保存。Interface 配置及完整映射集合也在同一事务中保存。校验、删除或写入任一步骤失败时，整笔保存回滚。删除 Provider 模型时，同时清理引用该 Provider 和上游模型组合的 Interface 映射。模型 CRUD 接口可以为兼容调用方保留，但必须复用相同的校验、父资源和引用清理规则。

## 鉴权

系统区分三类凭据：

- `ADMIN_TOKEN`：保护 `/api/*` 管理接口。配置后支持 `Authorization: Bearer` 和 `x-api-key`；未配置时管理接口以兼容模式开放，因此服务只能部署在本机或受控网络。单一管理令牌不承担用户、角色、登录、会话或多租户职责。
- Interface token：保护 `/v1/*`、`/models` 等协议入口。请求必须使用 Interface token，服务按该 Interface 的完整模型映射集合解析客户端模型名；Provider token 不能替代 Interface token。
- Provider API key：仅由服务端用于访问上游，不作为用户侧或管理侧凭据。

管理凭据和 Interface token 均支持以下请求头形式：

```text
Authorization: Bearer <token>
x-api-key: <token>
```

Interface token 只授予该 Interface 已配置模型的协议访问能力。模型不在该 Interface 的完整集合中时，请求失败，不回退到其他 Interface、Provider token 或旧 alias。

## 协议能力契约

### Chat Completions 到 Responses

该方向用于让 Codex 通过 Responses 入口调用 DeepSeek 或其他 OpenAI-compatible Chat Completions 上游，契约包括：

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

验收必须覆盖：

- Codex 配置 `wire_api = "responses"` 后能通过本服务调用 DeepSeek。
- 普通文本请求可用。
- 流式输出可用。
- 至少 function tool call 回合不崩。
- 请求日志能记录请求数、模型、provider、token、延迟、状态。

### 统计页面

统计形成请求观测闭环，范围包括：

- 统计 API。
- 管理页统计总览。
- 请求明细。
- 模型/provider 聚合。
- 价格表配置和成本估算。

验收必须覆盖：

- 能按日查看请求数、token、成本、成功率、延迟。
- 能定位失败请求的 provider、模型、协议和错误类型。
- 无价格配置的模型不会阻断请求。

### 原生 Responses 与 Anthropic Messages 上游接入

原生协议上游保持同协议语义，范围包括：

- `src/providers/responses.rs`。
- `src/providers/anthropic_messages.rs`。
- Responses 上游直通服务 `/v1/responses`。
- Anthropic Messages 上游直通服务 `/v1/messages`。
- 原生上游返回的 usage、stream event、request id、错误结构归一化到统计模型。
- 支持同协议直通和 agent 协议互转的内部响应模型。

验收必须覆盖：

- OpenAI Responses 原生上游可被 Codex 通过本服务调用。
- Anthropic Messages 原生上游可被 Claude Code / Anthropic-compatible 客户端通过本服务调用。
- 原生上游请求能写入统计日志。
- 原生上游不被降格转成 Chat Completions 出口。

### Chat Completions 到 Anthropic Messages

该方向用于让 Claude Code / Anthropic-compatible 客户端调用 Chat Completions 上游，范围包括：

- `/v1/messages`
- Anthropic Messages decoder / encoder
- Chat tool calls 到 `tool_use`
- `tool_result` 到 Chat `role=tool`
- Anthropic SSE 事件输出
- 统计复用。

验收必须覆盖：

- Claude Code 类客户端能通过本服务调用 DeepSeek / OpenAI-compatible 上游。
- 工具调用结构正确，不降级成纯文本。

### Responses 与 Anthropic Messages 互转

`responses <-> anthropic_messages` 只承载可以明确映射的 agent 语义，范围包括：

- Responses decoder 到 Internal。
- Anthropic decoder 到 Internal。
- 两端 encoder 复用。
- 严格禁止输出到 Chat Completions。

验收必须覆盖：

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

`previous_response_id` 重建历史会导致上下文增长。会话存储采用 LRU、TTL 和简单截断策略；摘要或持久化需要独立的生命周期与隐私设计，不在本契约中预设。

### 隐私和数据膨胀

统计默认只保存摘要，不保存完整 prompt 和响应。调试 payload capture 如果需要，必须单独开关、限时、脱敏。

### 参考项目复杂度

LiteLLM 和 Aura 的企业能力不应直接带入。本项目保持小型、可理解、可本地部署；新增平台能力必须对应已确认的运行约束。

## 组件职责

后端模块按路由、桥接模型、协议编解码、上游适配、统计和持久化分工：

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

前端统计视图及其 API 类型集中在：

```text
web/src/views/StatsView.vue
web/src/api/stats.ts
web/src/components/stats/*
```

## 验证策略

Rust 代码修改后执行：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Node.js / 前端命令使用 bun：

```text
bun test
bun run format:check
bun run lint
bun run typecheck
bun run build
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

`provider-relay` 是小型协议桥接网关：用户侧和供给侧均围绕 Chat Completions、Responses、Anthropic Messages 建立明确入口和适配器。核心转换通过内部模型承载，不做任意协议互转，禁止将入站 `responses` 和 `anthropic_messages` 降格输出到 `chat_completions`。

Interface token 和完整模型映射集合共同构成协议入口的授权与解析边界；Provider 与 Interface 保存均使用事务提交完整目标状态。统计只做观测，不做限额、预算和扣费。真实供应商与客户端兼容性必须通过单独的端到端记录确认。

# 协议桥接下一阶段规划

## 阶段背景

当前协议桥接已经完成第一版主链路：用户侧支持 Chat Completions、Responses、Anthropic Messages，供给侧支持 Chat Completions、Responses、Anthropic Messages，并且已经下线无法稳定支撑 Codex / Claude Code 复杂会话的 Ollama Native。

本规划启动后，已经完成第一批诊断和 metadata 收口：

- 后端提交 `f06bf64 记录桥接兼容诊断`，新增桥接兼容诊断模型和 request metadata schema，并把 Chat、Responses、Messages 请求日志接入 metadata / diagnostics。
- 前端提交 `7b907b0 展示请求诊断 metadata`，请求日志列表已经展示 diagnostics 摘要和 warning 标记。
- 工程治理提交 `82383dc 收口构建检查治理` 已移除 Cargo build script 自动修复动作，补齐 `format:check`、`.editorconfig`、`.gitattributes` 和 `.refs/` 忽略规则。
- 流式结构提交 `d0ae433 拆分流式桥接模块` 已将 `stream.rs` 拆成 stream 模块目录。

截至当前开发推进，阶段内代码项已经继续完成：

- Responses / Anthropic Messages 复杂流式互转已补齐工具调用增量、usage 事件和未知事件不中断处理。
- 流式统计已升级为固定 log id、首包插入和结束态 update，能够回填 completed、empty、final_usage_seen、stream_error、usage 和 tool_call_count。
- Stats API 已恢复返回 `metadata_json`，前端最近请求列表已支持 typed metadata 详情展开。
- 真实 Codex / Claude Code 客户端验收仍需在实际联调环境中补齐，当前本地验证仍以 mock upstream、单元测试和路由集成测试为主。

新的阶段目标不是继续堆单点补丁，而是把影响后续能力扩展的结构问题先处理掉。当前最明显的结构瓶颈已经收敛为：

- 流式桥接仍是 pairwise 转换，后续补工具调用增量、usage 聚合和统计会持续放大复杂度。
- 诊断 metadata 已经覆盖非流式请求日志，但 typed metadata、详情展示、流式 recorder 和真实客户端验收还没有闭环。

因此下一阶段后续工作应以“统一流式语义层”和“真实客户端验收”为主线，在已有 diagnostics / metadata 基础上补齐 Codex / Claude Code 真实使用所需的完整行为。

## 产品目标

下一阶段完成后，`provider-relay` 应具备以下能力：

- Codex 通过 Responses API 调用 Chat Completions 和 Responses 上游时，普通对话、工具调用、流式输出和错误统计可正常闭环。
- Claude Code / Anthropic-compatible 客户端通过 Messages API 调用 Chat Completions、Responses 和 Anthropic Messages 上游时，普通对话、工具调用、流式输出和错误统计可正常闭环。
- 桥接层不因 provider 能力声明或客户端扩展字段提前拒绝请求，优先执行映射、默认值补齐、跳过、文本化或透传。
- 每一次兼容处理都能进入结构化诊断，便于前端展示、日志审计和真实联调排查。
- 流式统计能覆盖首包时间、最终 usage、工具调用数量、上游 request id、空流和流中错误。

## 协议边界

下一阶段继续保持当前协议范围：

- 用户侧：`chat_completions`、`responses`、`anthropic_messages`。
- 供给侧：`chat_completions`、`responses`、`anthropic_messages`。

继续保持当前转换方向：

```text
chat_completions -> chat_completions
chat_completions -> responses
chat_completions -> anthropic_messages

responses -> responses
responses -> anthropic_messages

anthropic_messages -> anthropic_messages
anthropic_messages -> responses
```

暂不开放以下方向：

```text
responses -> chat_completions
anthropic_messages -> chat_completions
```

这个限制不是能力拒绝，而是产品边界。Responses 和 Anthropic Messages 通常承载 agent 客户端语义，不应降格包装成普通 Chat Completions 出口。

## 重构主线

### 评审问题并入范围

本阶段同时合并当前工程评审发现的问题，避免后续实施时继续沿着旧结构补丁式扩展。

需要一并处理的问题：

- Cargo 构建脚本自动修复问题已处理。当前 `build.rs` 执行 `bun run format:check`、`bun run lint` 和 `bun run build`，不再执行 `bun run format` 或 `bun run lint:fix`。后续只需要保持检查和修复命令分离。
- `src/bridge/stream.rs`、`src/routes/responses.rs`、`src/routes/messages.rs` 文件过大，且职责混合。流式解析、协议编码、路由调度、上游错误处理和日志写入需要拆分到明确模块。
- `request_logs.metadata_json`、Stats API 和前端 diagnostics 摘要已接通。剩余边界是 typed metadata、请求详情展示和流式 metadata 汇总，不再把基础暴露能力作为待办。
- 流式统计当前在首个 bytes chunk 到达时插入日志，缺少最终 usage、tool call 数、finish reason、空流和流中错误的结束态回填。
- 仓库已存在 `.editorconfig` 和 `.gitattributes`，`.refs/` 忽略规则也已提交。后续如果要调整换行或文本归一化，应作为独立格式治理任务处理，不和协议行为重构混在一起。

### 桥接诊断模型

已新增统一的桥接诊断模型，用来记录 decoder 中的兼容处理。当前基线已经覆盖 Responses、Anthropic Messages 和 Chat 请求日志的 diagnostics 写入。

剩余边界：

- stream decoder、stream encoder 和上游交互诊断仍需在流式中间层阶段接入。
- 诊断详情目前主要用于日志 metadata 和前端摘要，后续可在请求详情页展示更完整的 typed metadata。

当前对应模块：

```text
src/bridge/diagnostics.rs
```

核心结构方向：

```rust
pub struct DecodedRequest {
    pub request: InternalRequest,
    pub diagnostics: Vec<BridgeDiagnostic>,
}

pub struct BridgeDiagnostic {
    pub phase: DiagnosticPhase,
    pub protocol: String,
    pub path: String,
    pub action: DiagnosticAction,
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub original_kind: Option<String>,
}
```

建议枚举：

```rust
pub enum DiagnosticPhase {
    Decode,
    Encode,
    StreamDecode,
    StreamEncode,
    Upstream,
}

pub enum DiagnosticAction {
    Mapped,
    Defaulted,
    Ignored,
    Textified,
    PassedThrough,
}

pub enum DiagnosticSeverity {
    Info,
    Warning,
}
```

已接入或应保持诊断的兼容动作：

- 未知 role 映射为 `user`。
- 非 function tool 或缺少关键字段的 tool 被跳过。
- 非文本 content part 被 JSON 文本化。
- function call 缺少 `call_id` 时补默认 id。
- arguments 不是字符串时转为 JSON 字符串。
- 未识别流式事件被忽略或透传。

### 请求 metadata 构造器

不要在每个 route 里手写 `metadata_json`。当前已新增统一构造器：

```text
src/routes/request_metadata.rs
```

目标 schema：

```json
{
  "schema": "provider-relay.request_metadata.v1",
  "bridge": {
    "protocol_in": "responses",
    "protocol_out": "responses",
    "protocol_upstream": "chat_completions",
    "model_requested": "coder",
    "model_upstream": "deepseek-chat"
  },
  "diagnostics": [],
  "stream": {
    "empty": false,
    "completed": true,
    "final_usage_seen": false,
    "stream_error": null
  },
  "upstream": {
    "request_id": "req_123",
    "error_body_excerpt": null
  }
}
```

metadata 只保存协议、模型、路径、动作和摘要，不保存完整用户输入、工具参数和文件内容。

当前已完成：

- Chat、Responses、Messages 请求日志写入基础 bridge metadata。
- Responses、Messages decoder diagnostics 进入 request metadata。
- Stats API 返回 `metadata_json`。
- 前端请求日志展示 diagnostics 数量和 warning 标记。

剩余边界：

- typed metadata 还没有稳定暴露给前端详情层。
- 流式 metadata 仍缺少 completed、final usage、stream error、empty 等结束态汇总。

### 流式语义中间层

当前 `src/bridge/stream.rs` 应拆成模块目录，不继续维护 pairwise 状态机。

建议结构：

```text
src/bridge/stream/mod.rs
src/bridge/stream/sse.rs
src/bridge/stream/events.rs
src/bridge/stream/decode_chat.rs
src/bridge/stream/decode_responses.rs
src/bridge/stream/decode_anthropic.rs
src/bridge/stream/encode_responses.rs
src/bridge/stream/encode_anthropic.rs
src/bridge/stream/pipeline.rs
```

统一流程：

```text
上游 SSE bytes
  -> SSE frame parser
  -> upstream protocol decoder
  -> InternalStreamEvent
  -> stream recorder
  -> downstream protocol encoder
  -> 下游 SSE bytes
```

核心事件：

```rust
pub enum InternalStreamEvent {
    MessageStart { id: Option<String>, model: Option<String> },
    TextDelta { index: usize, delta: String },
    TextDone { index: usize },
    ToolCallStart { index: usize, id: Option<String>, name: Option<String> },
    ToolCallArgumentsDelta { index: usize, id: Option<String>, delta: String },
    ToolCallDone {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    Usage(StreamUsage),
    Finish { reason: Option<InternalFinishReason> },
    Error { code: Option<String>, message: String },
    RawPassthrough {
        protocol: &'static str,
        event: Option<String>,
        data: String,
    },
}
```

### 流式统计 recorder

现有 `record_first_chunk()` 只能在第一块 bytes 到达时插入日志。下一阶段应升级为事件层 recorder：

- 首个语义输出记录 `first_token_ms`。
- `Usage` 事件累计 `input_tokens`、`output_tokens`、`reasoning_tokens`。
- `ToolCallStart` 或 `ToolCallDone` 统计 `tool_call_count`。
- `Finish` 标记 `completed = true`。
- 空流记录 `stream.empty = true`。
- 流中错误记录 `stream_error`。
- 上游 request id 进入 metadata 和 request log 字段。

实现上可以先继续“首包插入日志”，但最终目标应支持“固定 log id + 结束时更新”。如果当前 schema 不便更新，可以先在 metadata 中保留 stream 汇总，再单独评估是否给 `request_logs` 增加更新函数。

## 阶段拆分

### 阶段目标：诊断与 metadata

状态：主干已完成，后续只保留详情展示和流式 metadata 边界。

已完成：

- 新增 `BridgeDiagnostic`、`DecodedRequest`。
- Responses decoder 记录兼容动作。
- Anthropic Messages decoder 记录兼容动作。
- Chat 路由接入基础 bridge metadata。
- 新增 `RequestMetadataBuilder`。
- `routes/responses.rs`、`routes/messages.rs` 和 `routes/chat.rs` 写入 metadata。
- Stats API 返回 `metadata_json`。
- 前端请求日志展示诊断数量和 warning 标记。
- 不在 route 中手写散落的 metadata JSON，统一走 builder。

剩余边界：

- typed metadata 详情展示。
- 流式路径的 diagnostics 和结束态 stream metadata。
- 真实客户端联调中新增字段或事件的诊断补充。

验收标准：

- 未知 role、未知 tool、非文本内容不导致请求失败。已由后端诊断测试覆盖。
- 成功请求和上游失败请求保留 decode diagnostics。已接入请求日志 metadata。
- `/api/stats/requests` 能返回 metadata。已完成。
- 前端能看到请求是否发生过兼容处理。已完成摘要展示。

### 阶段目标：流式中间层

目标：拆掉 pairwise stream 转换，建立 `InternalStreamEvent`。

主要任务：

- 抽 `SseFrame` parser 和 encoder。
- 新增 `InternalStreamEvent`、`StreamUsage`、`InternalFinishReason`。
- Chat SSE decoder 产出内部事件。
- Responses SSE encoder 消费内部事件。
- Anthropic Messages SSE encoder 消费内部事件。
- 用新 pipeline 替换 `chat_sse_response_to_responses_sse`。
- 用新 pipeline 替换 `chat_sse_response_to_anthropic_messages_sse`。
- 同步拆分 `stream.rs` 的通用 SSE 解析、上游 decoder、下游 encoder 和 pipeline 组合职责。

验收标准：

- Chat Completions 流式文本到 Responses SSE 行为保持不变。
- Chat Completions 流式工具调用到 Responses SSE 行为保持不变。
- Chat Completions 流式文本和工具调用到 Anthropic Messages SSE 行为保持不变。
- 现有流式路由测试全部通过。

### 阶段目标：跨 agent 流式补齐

目标：补齐 Responses 和 Anthropic Messages 之间的复杂流式互转。

主要任务：

- Responses SSE decoder 支持文本、function call added、arguments delta、arguments done、output item done、usage、completed。
- Anthropic Messages SSE decoder 支持 message_start、content_block_start、text_delta、tool_use、input_json_delta、content_block_stop、message_delta usage、message_stop。
- Responses -> Anthropic Messages 支持工具调用流式转换。
- Anthropic Messages -> Responses 支持工具调用流式转换。
- 未识别事件进入 diagnostics，不直接失败。

验收标准：

- `responses -> anthropic_messages` 支持流式工具调用。
- `anthropic_messages -> responses` 支持流式工具调用。
- usage 能进入 `InternalStreamEvent::Usage`。
- 未识别事件不会中断流。

### 阶段目标：流式统计补齐

目标：让统计从“请求级粗统计”变成“流式语义统计”。

主要任务：

- 新增 stream recorder。
- 累计 usage、tool call 数、finish reason。
- 空流和流中错误进入 metadata。
- 上游 request id 在流式路径中尽量回填。
- Stats API 和前端展示流式诊断。
- 评估 request log 的固定 id 和结束更新函数，避免首包插入后无法回填最终统计。

验收标准：

- 流式请求有 `first_token_ms`。
- 带 usage 的 SSE 最终能回填 token。
- 流式 tool call 能记录 `tool_call_count`。
- 空流和流中错误能在日志里区分。

### 阶段目标：真实客户端验收

目标：证明 Codex / Claude Code 真实可用。

最小验收矩阵：

```text
Codex -> responses -> DeepSeek Chat Completions
Codex -> responses -> OpenAI Responses
Claude Code -> messages -> DeepSeek Chat Completions
Claude Code -> messages -> Anthropic Messages
Claude Code -> messages -> OpenAI Responses
```

每条路径至少验证：

- 普通对话。
- 流式输出。
- 工具调用。
- 上游错误。
- request log 落库。
- metadata diagnostics。
- stats 聚合展示。

验收材料：

- 每条链路的配置样例。
- 请求入口、上游协议、模型映射。
- 成功和失败日志截图或 JSON 摘要。
- 已知降级项和后续处理项。

### 阶段目标：工程治理收口

目标：让开发、检查和提交过程可审计，避免工具链自动修改源码。

已完成：

- `build.rs` 已移除 `bun run format` 和 `bun run lint:fix` 这类自动修复动作。
- 前端已新增 `format:check`，自动修复保留在显式开发命令。
- 仓库已存在 `.editorconfig` 和 `.gitattributes`。
- `.refs/` 忽略规则已提交，参考项目目录不会进入版本控制。

剩余任务：

- 明确 Rust 检查、前端检查和生产静态资源构建的边界。

验收标准：

- `cargo clippy`、`cargo test` 不会因为 build script 自动格式化或 lint fix 修改工作树。当前配置已满足。
- 前端存在 check-only 的格式检查入口。当前配置已满足。
- `.refs/` 保持未跟踪，不进入提交。当前配置已满足。
- 文档、Rust、前端文件的基础文本治理规则明确。

## 重构边界

### 必须重构

- `src/bridge/stream.rs` 拆成 stream 模块目录。
- 流式 decoder 返回 diagnostics 并进入 request metadata。
- metadata 构造继续保持 builder，不回退到 route 手写 JSON。
- 流式统计从 bytes 首包记录升级为语义事件记录。

### 暂不重构

- 不开放 `responses -> chat_completions` 和 `anthropic_messages -> chat_completions`。
- 不引入复杂 provider trait 框架。
- 不做限额、预算、扣费。
- 不重新引入 Ollama Native。
- 不把完整原始请求写入 metadata。

## 测试策略

每个阶段都应补对应测试，不等到最后统一补。

后端测试：

- decoder 兼容诊断单测。已覆盖首批非流式诊断。
- request metadata builder 单测。已覆盖首批 schema 构造。
- stream SSE parser 单测。
- stream decoder / encoder 单测。
- 路由集成测试。
- stats metadata 返回测试。已覆盖 metadata 返回。

前端测试：

- 请求日志 metadata 字段类型定义。已完成。
- 诊断数量和 warning 展示。已完成。
- 无 metadata 的旧日志兼容显示。已完成。

验证命令：

```powershell
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

如果改动前端：

```powershell
bun run lint
bun run build
```

## 风险与处理

### 流式事件语义不完全一致

Responses output index、Anthropic content block index、Chat tool call index 不完全等价。encoder 必须维护自己的下游 index 状态，不能简单透传上游 index。

### Tool call 参数不是完整 JSON

流式 arguments delta 只是字符串片段，不能强制每段都解析 JSON。只能累计字符串，在 done 阶段整体保留。

### metadata 泄露用户内容

diagnostics 只记录路径、动作、类型摘要和短消息，不记录完整 prompt、文件内容和工具参数。

### 统计写库时机变化

如果从首包插入改为结束插入，长流式请求期间前端可能看不到进行中记录。可以先保留首包插入，再增加结束更新。

### 真实客户端行为超出 mock

真实 Codex / Claude Code 可能发送 mock 没覆盖的事件或字段。所有 decoder 都应保留 `RawPassthrough` 或 diagnostics，不应因为未知扩展中断请求。

## 交付节奏

建议按五个可提交逻辑单元推进：

- 提交一：新增桥接诊断和 request metadata，接入非流式路径。已完成，后端提交 `f06bf64`，前端提交 `7b907b0`。
- 提交二：拆分 stream 模块，引入 `InternalStreamEvent`，迁移 Chat SSE 到 Responses / Anthropic 的现有能力。
- 提交三：补齐 Responses / Anthropic 流式工具调用和 usage。
- 提交四：补流式统计、typed metadata 详情展示和真实客户端验收记录。
- 提交五：收口工程治理，拆掉 build script 自动修复动作，补齐 check-only 前端脚本和文本治理配置，并提交 `.refs/` 忽略规则。已完成，提交 `82383dc`。

每个提交都要求：

- 范围独立，可回滚。
- 有对应测试。
- 不引入新的硬拒绝能力判断。
- 不把 `.refs` 参考目录纳入提交。

## 下一步执行建议

下一步进入“流式中间层”。

原因：

- 诊断和 request metadata 主干已经落地，可以承接后续 stream diagnostics 和 stream metadata。
- 当前最大风险仍在 pairwise 流式转换，继续在 `stream.rs` 上补事件会放大状态机复杂度。
- 流式 recorder、usage、tool call、finish reason 和真实客户端验收都依赖统一的 `InternalStreamEvent`。

建议先迁移 Chat SSE 到 Responses / Anthropic 的既有能力，保持行为不变；迁移稳定后，再补 Responses / Anthropic 之间的复杂流式工具调用和 usage。

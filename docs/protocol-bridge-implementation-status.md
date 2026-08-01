# 协议桥接实现验收与差距清单

本文记录当前实现相对 `docs/protocol-bridge-gateway-design.md` 的落地状态。结论基于当前源码结构和现有测试用例观察，不等同于真实客户端联调完成。

## 当前结论

协议桥接主体已经从透明代理演进为多入口、多上游协议的桥接网关。当前产品范围收敛为 `chat_completions`、`responses` 和 `anthropic_messages`。Ollama Native 因无法稳定支撑 Codex / Claude Code 的工具调用和复杂会话目标，已从协议入口、上游类型、模型目录和前端 provider 选项中下线。

源码和现有测试用例覆盖跨 agent 流式工具调用、流式 usage 统计回填和 typed metadata 详情展示。当前证据仍不能等同于完整产品验收，主要边界集中在两类：

- 现有验证以 mock upstream 和单元测试为主，缺少 Codex、Claude Code、DeepSeek、OpenAI、Anthropic 的真实客户端联调记录。
- 原生 Responses / Anthropic Messages 流式直通仍不解析上游 usage 事件；已通过桥接语义层的流式路径可以回填 usage、tool-call 数量、完成状态、空流和流中错误。

## 已实现范围

### 用户侧协议入口

已实现的用户侧入口：

- `/v1/chat/completions`：普通 OpenAI Chat Completions 入口。
- `/v1/responses`：OpenAI Responses 入口，面向 Codex 类客户端。
- `/v1/messages`：Anthropic Messages 入口，面向 Claude Code / Anthropic-compatible 客户端。
- `/v1/models` 和 `/models`：模型目录入口，返回 provider、上游协议、上游模型、可用下游协议和能力声明。

### 管理配置与事务边界

源码中的 Provider 和 Interface 保存接口使用完整目标状态：

- Provider 请求的 `models` 表示保存后的完整模型集合。创建时必须提供；更新时提供该字段则替换完整集合，缺省则保留已有集合。
- Interface 请求的 `models` 表示保存后的完整模型映射集合。每项引用 Provider 及其已保存的上游模型，并定义客户端使用的模型名。
- Provider 配置与模型集合在一个 SQLite 事务中保存；Interface 配置与模型映射集合也在一个事务中保存。校验或写入失败时回滚，不保留部分结果。
- 删除 Provider 模型时按 Provider 和上游模型组合清理 Interface 引用；删除 Interface 模型时同时匹配父 Interface 和模型 ID。

现有管理路由测试用例覆盖创建、完整集合替换、非法引用回滚、空集合拒绝和跨 Interface 删除边界。这些是源码与本地自动化测试层面的证据，不代表浏览器管理流程已经完成交互验收。

### 鉴权边界

协议入口要求 Interface token，可通过 `Authorization: Bearer` 或 `x-api-key` 传入。解析链先按 token 定位 Interface，再在该 Interface 的完整模型集合中解析客户端模型名；缺少 token、使用 Provider token 或模型只存在于其他 Interface 时均不能通过该链路。

`ADMIN_TOKEN` 只保护 `/api/*` 管理接口。配置后同样支持 Bearer 和 `x-api-key`；未配置或为空时管理 API 以兼容模式开放。该兼容模式和单一管理令牌只适用于本机或受控网络，没有引入用户、角色、登录、会话或多租户系统。

### 供给侧协议模型

`ProviderSpec` 当前把上游分为三类：

- `responses`
- `chat_completions`
- `anthropic_messages`

当前 provider 类型映射大致为：

- `openai` 映射为原生 Responses 上游。
- `openai_compatible` 映射为 Chat Completions 上游。
- `anthropic` 和 `anthropic_compatible` 映射为 Anthropic Messages 上游。
- 未识别 provider 默认按 Chat Completions 处理。
- provider 能力模型已覆盖工具调用、reasoning、tool choice、并行工具调用、system message、结构化输出、流式 usage，以及上下文和输出 token 上限。
- provider 能力声明用于模型目录展示、诊断和后续兼容策略，不作为主链路硬拒绝条件。

### 已实现的协议方向

当前已落地的方向：

- `chat_completions -> chat_completions`
- `chat_completions -> responses`
- `chat_completions -> anthropic_messages`
- `responses -> responses`
- `responses -> anthropic_messages`，支持非流式、文本流式、流式工具调用和 usage 事件。
- `anthropic_messages -> anthropic_messages`
- `anthropic_messages -> responses`，支持非流式、文本流式、流式工具调用和 usage 事件。

### 流式桥接

已实现的流式路径：

- Chat Completions 上游流式透传到 `/v1/chat/completions`。
- Chat Completions 上游流式转换为 Responses SSE。
- Chat Completions 上游流式转换为 Anthropic Messages SSE。
- 原生 Responses 上游流式直通到 Responses 入口。
- Responses 上游文本流式、function call 增量和 usage 事件转换为 Anthropic Messages SSE。
- 原生 Anthropic Messages 上游流式直通到 Messages 入口。
- Anthropic Messages 上游文本流式、tool_use 增量和 usage 事件转换为 Responses SSE。

当前跨 agent 流式桥接已经覆盖工具增量和 usage 聚合。未知 SSE 事件会被忽略并继续处理后续事件，但还没有把 Raw/diagnostic 事件正式写入 metadata。

### 会话和工具调用

已实现内容：

- Responses 请求支持 `previous_response_id` 本地会话链。
- 非流式 Responses 响应会保存会话，用于后续工具回合恢复上下文。
- Chat Completions 工具调用可以编码到 Responses 输出。
- Anthropic `tool_use` / `tool_result` 与内部工具模型已有基础互映。
- 桥接层优先尝试映射、透传和降级，不因 provider 能力声明不完全匹配而提前拒绝。

仍需补齐的内容：

- DeepSeek reasoning / thinking 回放只具备设计预期，当前没有看到完整 provider 特化处理闭环。
- 流式 tool-call 的跨协议状态机已补齐本地实现和 mock 测试，真实客户端复杂会话仍需验收。
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
- 请求 metadata 详情展开，展示 bridge 协议/模型、diagnostics 明细、stream 状态和 upstream 摘要。

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

当前统计仍有边界：

- 流式请求已采用固定 log id，首包插入后在结束时回填 completed、empty、final_usage_seen、stream_error、usage 和 tool_call_count。
- `first_token_ms` 字段已建模和聚合展示，流式路径会在首个 bytes chunk 到达时写入。
- `upstream_request_id` 字段已建模和展示，但多数 provider 响应路径没有从响应头或响应体提取。
- 上游错误多数只记录 HTTP 状态，没有统一解析 provider error code、error message 和 request id。
- 原生 Responses / Anthropic Messages 流式直通时，usage 事件没有被解析并归一化到统计字段。
- 成本估算依赖本地价格配置，无法匹配价格时只能保持空值；这不是阻塞问题，但需要在产品界面明确显示。

## 构建与部署边界

构建契约要求先在 `web/` 中执行 `bun install --frozen-lockfile`。Cargo 的 `build.rs` 监听 `web/bun.lock`，但不隐式安装依赖；`web/node_modules` 缺失时直接失败并给出冻结安装命令。前端脚本分别提供 `test`、`typecheck` 和 `build` 入口，Docker 构建上下文排除根目录及 `web/` 下的 `.env` 文件。

这些契约用于保证本地和 Docker 构建使用同一锁文件。本文没有记录 Docker 镜像构建或容器运行结果，因此不能据此宣称镜像已经验收。

## 验收状态

### 已具备本地验收基础

现有测试和代码已经覆盖以下基础行为：

- 协议入口鉴权。
- 模型别名解析。
- 模型别名按下游协议过滤。
- Chat Completions 非流式和流式代理。
- Responses 到 Chat Completions 上游。
- Responses 到原生 Responses 上游。
- Responses 到 Anthropic Messages 上游的非流式和文本流式桥接。
- Anthropic Messages 到 Chat Completions 上游。
- Anthropic Messages 到原生 Anthropic Messages 上游。
- Anthropic Messages 到 Responses 上游的非流式和文本流式桥接。
- Responses 到 Anthropic Messages 上游的流式工具调用和 usage 事件桥接。
- Anthropic Messages 到 Responses 上游的流式工具调用和 usage 事件桥接。
- 流式 request log 的首包插入、结束态回填、空流和流中错误 metadata。
- `previous_response_id` 多跳历史。
- function tool call 回合。
- 统计 API 聚合。

这些验收依赖本地 mock upstream，能证明内部转换和路由分支基本成立，但不能证明真实客户端兼容性。

### 尚未完成的真实联调

需要补充真实联调记录：

- Codex 使用 `wire_api = "responses"` 通过本服务调用 DeepSeek Chat Completions 上游。
- Codex 通过本服务调用 OpenAI 原生 Responses 上游。
- Claude Code / Anthropic-compatible 客户端通过本服务调用 DeepSeek Chat Completions 上游。
- Claude Code / Anthropic-compatible 客户端通过本服务调用 Anthropic 原生 Messages 上游。
- Claude Code / Anthropic-compatible 客户端通过本服务调用 OpenAI Responses 上游的非流式桥接。
- 普通 OpenAI-compatible 客户端通过 `/v1/chat/completions` 调用 Chat Completions 上游。

真实联调需要至少记录：

- 客户端配置。
- provider 配置。
- 请求是否流式。
- 是否包含工具调用。
- 响应是否能被客户端正确消费。
- `request_logs` 是否记录正确的协议、provider、模型、状态、token 和错误信息。

## 与设计文档的差距

### 协议方向差距

设计要求的主要协议方向已实现。`responses <-> anthropic_messages` 已支持非流式、文本流式、工具调用增量和 usage 聚合的本地实现。

设计中禁止的方向仍保持关闭：

- 入站 `responses` 没有被降格输出到 Chat Completions。
- 入站 `anthropic_messages` 没有被降格输出到 Chat Completions。

### 流式状态机差距

当前已有多条流式文本路径，跨 agent 协议互转已经具备工具调用和 usage 事件状态机：

- Responses SSE 的 function_call added、arguments delta/done、output item done、usage、completed 已进入内部事件层。
- Anthropic Messages SSE 的 tool_use、input_json_delta、content_block_stop、message_delta usage、message_stop 已进入内部事件层。
- 未知 SSE 事件当前按忽略处理，不中断后续流；Raw/diagnostic metadata 仍是后续增强项。

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

能力画像当前不作为硬拒绝条件。后续应改造成兼容计划输入：优先映射，其次补默认值或降级，再记录诊断。只有上游真实返回错误或协议状态无法闭合时，才以失败请求落库。

### 错误和审计差距

当前错误处理能落库失败状态，但审计质量还不够：

- 上游错误体没有统一结构化。
- 上游 request id 没有普遍提取。
- 兼容性降级、字段忽略和上游失败的错误分类还不够细。
- 流式中途失败已通过 stream recorder 回填失败状态和 `stream.stream_error`。

## 建议优先级

### 先收口真实客户端验收

先补真实联调，而不是继续扩展协议面。当前最能暴露问题的是：

- Codex + DeepSeek Chat Completions，上游走 `/v1/responses`。
- Claude Code + DeepSeek Chat Completions，上游走 `/v1/messages`。
- Codex + OpenAI Responses，原生上游直通。
- Claude Code + Anthropic Messages，原生上游直通。

### 再补真实统计边界

统计已补齐桥接流式路径的结束态回填。后续重点是原生透传和真实上游字段提取：

- 提取上游 request id。
- 原生 Responses / Anthropic Messages 直通流解析 usage。
- 明确无法统计的字段为空，而不是估算成错误值。

### 最后推进真实客户端验收

`responses <-> anthropic_messages` 跨 agent 流式增强已完成本地实现。后续应使用真实 Codex / Claude Code 客户端做端到端验收。

## 当前可发布边界

如果按内部试用口径，当前可以作为协议桥接网关的早期版本使用：

- 支持 Codex 通过 Responses 调用 Chat Completions 和 Responses 上游。
- 支持 Claude Code 类客户端通过 Messages 调用 Chat Completions 和 Anthropic Messages 上游。
- 支持普通 OpenAI-compatible 客户端调用 Chat Completions 上游。
- 支持基础统计页面和请求日志。

如果按对外产品口径，仍不应宣称完整支持：

- 不应宣称已完成 Codex / Claude Code 真实兼容性认证。
- 不应宣称统计 token、成本、首 token、上游 request id 在所有原生透传协议上都准确。
- 不应宣称支持限额、预算、计费或企业审计。

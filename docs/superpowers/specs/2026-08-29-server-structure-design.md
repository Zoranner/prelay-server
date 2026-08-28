# 服务端目录结构重构设计

## 目标

将当前服务端从以大型平级文件和文件名前缀承载职责的结构，重构为由目录表达领域、协议和方向边界的结构。重构完成后，所有 `src/` 与 `tests/` 下的 Rust 源文件物理行数不得超过 450 行。

本次只调整模块位置、可见性和测试组织；公开的 `/api/*`、`/v1/*`、管理 DTO、稳定错误码、数据库表名与 SQLite/PostgreSQL 语义均不得变化。

## 结构原则

- 目录先表达稳定职责，文件名只描述该目录内的单一实现，不再用 `identity_`、`encode_`、`decode_`、`extensions_`、`schema_` 等前缀模拟层级。
- 协议适配按协议归属；流式适配再按编码和解码方向归属。不同协议的语义不因目录重构而被强行合并。
- 公开路由按请求协议归属；每个协议目录自己维护入口、候选执行、上游调用、观测和私有测试。仅在行为与模型确实一致时提取公共模块。
- `Storage` 保留为调用方使用的单一类型，但其方法实现分散到所属领域模块；`storage/mod.rs` 只保留状态、错误、共享类型和模块声明。
- 目录重组不得扩大 `pub` 可见性。跨同级模块的最小共享使用 `pub(crate)` 或 `pub(super)`，不新增平行抽象、迁移路径或兼容入口。

## 目标目录

```text
src/
  bridge/
    anthropic/
      mod.rs
      decode.rs
      encode.rs
    responses/
      mod.rs
      decode.rs
      encode.rs
    stream/
      decode/
        anthropic.rs
        chat.rs
        responses.rs
      encode/
        anthropic.rs
        responses.rs
      events.rs
      pipeline.rs
      sse.rs
      mod.rs
    diagnostics.rs
    internal.rs
    mod.rs
  entity/
    identity/
      provider_configs.rs
      provider_models.rs
      endpoint_configs.rs
      endpoint_models.rs
      endpoint_model_routes.rs
      model_aliases.rs
      request_logs.rs
      response_sessions.rs
      mod.rs
    identities.rs
    mod.rs
  observability/
    stream_stats/
      mod.rs
      record.rs
      state.rs
      persistence.rs
      tests.rs
    request_metadata.rs
    upstream_observability.rs
    mod.rs
  providers/
    chat_completions/
      request.rs
      response.rs
      stream.rs
      mod.rs
    spec/
      capabilities.rs
      urls.rs
      mod.rs
    anthropic_messages.rs
    model_discovery.rs
    responses.rs
    mod.rs
  routes/
    v1/
      chat/
      images/
      messages/
      responses/
      auth.rs
      endpoint_resolver.rs
      models.rs
      mod.rs
  schema/
    tables/
    indexes.rs
    mod.rs
  storage/
    access.rs
    identities.rs
    endpoints.rs
    providers.rs
    sessions.rs
    request_logs.rs
    stats.rs
    crypto.rs
    mod.rs
tests/
  extensions/
  identity/
  management/
  schema/
  v1/
  support/
    mod.rs
    auth.rs
    http.rs
    status.rs
  test_context/
    mod.rs
  extensions.rs
  identity.rs
  management.rs
  schema.rs
  v1.rs
  source_layout.rs
```

空目录不先创建。每个目录只在迁入首个真实模块时建立；如果一个职责最终只保留一个短文件，则不再人为增加一层目录。

## 桥接与 Provider 协议

`bridge/anthropic/` 与 `bridge/responses/` 分别包含该协议到内部模型的解码和从内部模型回到该协议的编码。`bridge/stream/decode/`、`bridge/stream/encode/` 只包含流式协议方向转换，公共 SSE、事件和 pipeline 继续在 `stream/` 根层。

`providers/chat_completions/` 按请求编码、响应解码和 SSE 文本提取拆分；`providers/spec/` 按能力解析和上游 URL 解析拆分。所有既有导出函数由各自的 `mod.rs` 重新导出，路由与上游调用方不改变行为。

## 协议路由

`routes/v1/chat/`、`images/`、`messages/` 和 `responses/` 各自包含小型路由注册、输入处理、候选执行、协议专属上游调用、请求日志与流式处理。候选循环、`remember_protocol_model_provider`、重试策略和失败语义保持原样；在多个协议中完全相同的无状态操作才进入 `routes/v1/` 公共模块。

各协议的私有测试按认证、候选切换、路由、流式转换、请求日志、会话或工具调用等实际行为组织。`tests/mod.rs` 只保留共享导入、fixture 组织和私有子模块声明；fixture 放在 `tests/fixtures.rs`，不使用 `include!` 拼接测试片段。

不得创建“通用协议 handler”以抹平 Responses、Chat Completions、Anthropic Messages 与图像生成的不同请求、响应和流式语义。

## 流观测

`observability/stream_stats/` 将公开记录入口、单请求状态、持久化辅助和测试分开。`record.rs` 保留 `record_first_chunk` 与 `record_stream` 的既有签名，目录根只重新导出这两个入口；`state.rs` 继续维护首块、结束、错误和最终 usage 的原有状态流转；`persistence.rs` 只封装请求日志写入和脱敏失败日志。

该目录迁移是为补齐全树 450 行门禁而插入的结构任务，不改变流式响应内容、请求日志字段、错误语义或存储接口，也不新增跨协议观测抽象。

## 持久化与实体

当前 `identity_*.rs` 实体迁入 `entity/identity/`，路径表达身份作用域下的 Provider、接入点、模型、请求日志与会话关系。SeaORM 的表名、实体字段、关系定义和现有调用结果不得改变。

`schema/` 将初始化编排、表创建和索引定义分离。初始化仍是单一入口，按当前数据库方言决定 SQL；不得引入迁移工具或旧数据库兼容。

`Storage` 的身份、Provider、接入点、协议访问、会话、请求日志和统计方法分别放入对应模块。调用方仍通过同一个 `Storage` 实例访问，事务边界、加密边界和跨身份隔离保持不变。

## 集成测试与结构门禁

Cargo 只将 `tests/` 根层文件识别为集成测试 target，因此根层保留短的 `extensions.rs`、`identity.rs`、`management.rs`、`schema.rs` 和 `v1.rs` 作为模块入口；入口使用显式 `#[path]` 声明同名目录中的领域文件，不包含测试逻辑。实际测试按领域放在同名目录。

`tests/support/mod.rs` 保留数据库与环境 fixture，`auth.rs`、`http.rs` 和 `status.rs` 分别保留注册认证、JSON 请求和无响应体状态请求工具。各 integration target 只声明实际需要的 support 模块；support 不承载 Provider、接入点、统计等具体资源域断言。

身份存储测试由 `tests/identity/storage/mod.rs` 组织，按凭据、事务、候选排序、会话作用域和主密钥拆分；共享输入和数据构造放在同目录 `fixtures.rs`。入口不承载测试或 fixture 实现。

新增 `tests/source_layout.rs`，递归检查 `src/` 与 `tests/` 的 Rust 文件物理行数，并检查已迁移区域不得重新出现用于表达下级模块的旧前缀。该测试先以当前结构失败，再随每批迁移逐步缩小失败集合，最终作为长期门禁。

## 实施顺序

先建立结构门禁和桥接目录，再迁移 Provider 协议与 `/v1` 路由。随后迁移实体、schema 与 Storage，并在重组集成测试前补充分流统计观测模块，消除既有超长源码；最后重组集成测试。每个批次在提交前都必须满足受影响目录内无文件超过 450 行，并运行对应的聚焦测试；最终执行 `cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features` 与全树结构门禁。

由于当前仓库只保留一个工作树，重构直接在现有 `master` 工作树分批提交；不创建额外工作树，也不触及 `prelay-client` 或 `prelay-protocol`。

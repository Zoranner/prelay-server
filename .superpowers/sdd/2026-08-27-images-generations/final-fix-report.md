# 最终审查修复报告

## 范围与基线

- 基线提交：`7ce90f4f93208cce70eb734ab2e55674ca36a854`
- 需求依据：`final-fix-brief.md`
- 修改范围：图像生成路由、ProviderSpec 协议选择、`/v1/models` 候选协议聚合及对应测试。
- 未修改协议仓、客户端、部署、数据库 schema 和既有未跟踪的 `docs/superpowers/`。

## 修复结果

- 图像请求日志改为尽力写入。日志持久化失败只产生固定 `image_request_log_storage` 告警，不再阻断可重试候选切换，也不再丢弃已经取得的非流式响应状态、Content-Type 和原始字节。
- 上游连接失败和非流式响应体读取失败均写入 `images_generations` 失败记录，错误类别分别为 `upstream_connection` 和 `upstream_body`。公开错误与请求记录只保留固定安全消息，不包含 Provider URL、提示词、Endpoint Token、Provider API Key、图片 URL 或 Base64。
- 流式首包测试使用 `tokio::sync::Notify` 控制第二个事件。测试在首事件到达后显式释放上游，再核对第二事件、EOF 和完整原始字节顺序，不再依赖 200 ms 与 250 ms 的窄时钟差。
- 单一显式协议覆盖会同步 ProviderSpec 主协议；`/v1/models` 保留同名模型首候选的展示字段，同时按候选顺序合并全部下游协议并去重。

## RED 与 GREEN

### 请求日志写入失败

- RED：`cargo test request_log_cannot_be_written`，2 项失败。SQLite trigger 强制拒绝 `identity_request_logs` INSERT 后，现有代码将日志错误传播为 `Protocol/Internal`，分别阻断候选切换和成功响应返回。
- GREEN：同一命令 2 项通过。500 主候选仍切换到备用候选；成功响应保留原始 201 状态、Content-Type 和字节。

### 连接失败与响应体中断

- RED：`cargo test logs_sanitized_failure`，2 项失败。公开消息分别包含完整请求 URL 和底层响应体解码错误，且失败记录尚未写入。
- GREEN：同一命令 2 项通过。两类错误均保持 `AppError::Upstream { status: None }`，并留下固定类别的脱敏失败记录。

### 流式首包确定性

- 原测试问题：上游固定等待 250 ms，客户端要求 200 ms 内收到首事件，结果依赖调度时钟差。
- 替换验证：`cargo test streams_image_events_without_waiting_for_upstream_done`，1 项通过。上游在首事件后等待通知，客户端先收到首事件，再显式释放并核对完整事件顺序与 EOF。

### 模型协议元数据

- RED：`cargo test providers::spec::tests::enables_image_generations_only_when_explicitly_declared`，1 项失败，实际主协议为 `Responses`，预期为 `ImageGenerations`。
- GREEN：同一命令 1 项通过。
- RED：`cargo test routes::v1::models::tests`，新增的 2 项失败。仅图像候选错误报告 `chat_completions`；文本加图像同名候选缺失 `images_generations`。
- GREEN：同一命令 4 项通过，既有 2 项与新增 2 项全部通过。

## 最终验证

- `cargo fmt --all`：通过。
- `cargo clippy --all-targets --all-features -- -D warnings`：通过，无 warning。
- `cargo test routes::v1::images::tests`：10 项通过。
- `cargo test routes::v1::models::tests`：4 项通过。
- `cargo test providers::spec::tests`：21 项通过。
- `cargo test --all-targets --all-features`：235 项通过，0 项失败，1 项按既有条件忽略。
- `git diff --check`：提交前执行并记录最终结果。

## 提交与剩余边界

- 提交标题：`修复图像中继观测与模型协议元数据`。本报告与修复代码、回归测试在同一提交中收口。
- 未配置 `TEST_POSTGRES_URL`，因此需要空 PostgreSQL 测试库的 schema 初始化测试仍按既有条件忽略；本次未修改 schema。
- 未执行真实供应商调用、部署、推送、发布或标签操作。

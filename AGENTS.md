# prelay-server

`prelay-server` 是 Rust/Axum 协议桥接服务：按身份隔离配置，以 `ENCRYPTION_KEY` 加密保存 Provider API Key，执行上游调用，并暴露管理 API 与 `/v1` 协议入口。

## 主链路与职责

- 管理侧：`POST /api/identities` 是唯一匿名入口；其余 `/api/*` 由 device credential 认证，并且只能访问当前身份的 Provider、Endpoint、统计和凭据轮换。
- 调用侧：`/v1/models`、`/v1/responses`、`/v1/chat/completions`、`/v1/messages` 由 Endpoint Token 认证。模型必须先在该 Endpoint 的映射中解析，再按 `candidate_order` 选择兼容的 Provider。
- Provider 保存模型和 Endpoint 映射均采用完整目标状态与事务。不要用局部补丁绕过引用校验，或让跨身份配置相互可见。
- 失败重试、候选切换、超时和最多候选数是服务端部署策略，通过 `UPSTREAM_*` 环境变量配置；不要将它们变成桌面端的终端用户设置。
- 请求统计只保存观测所需元数据、状态和用量，不默认保存完整 prompt 或响应正文。不要把统计扩展为配额、计费或限流，除非任务明确要求。

## 数据与安全边界

- `data/relay.db` 是运行时 SQLite 数据库；不提交数据库、`.env`、主密钥、设备凭据、Endpoint Token 或 Provider API Key。
- 旧版未按身份归属的数据库会被有意丢弃，不能在未重新定义迁移和密钥边界的情况下恢复兼容路径。
- 服务本身不提供 TLS。任何暴露到非受信网络的部署必须在运行环境中提供 TLS 与网络访问控制；不要把该部署责任伪装成路由层功能。
- 不终止用户正在运行的 `prelay-server.exe`。若 Cargo 因 Windows 文件锁失败，报告锁和命令，不使用外部 target 目录规避。

## 开发与验证

- 首次获取源码或协议变更后，执行 `git submodule update --init --recursive`。
- 本地运行需要有效的 `ENCRYPTION_KEY`；可由仓库根 `.env` 加载。默认监听 `0.0.0.0:18080`，`LISTEN_PORT` 可覆盖。
- 修改 Rust 代码后在仓库根目录执行：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
```

- 修改数据库初始化、原始 SQL、SeaORM 映射或 PostgreSQL 连接配置时，除上述检查外，必须将 `TEST_POSTGRES_URL` 指向独立的全新 PostgreSQL 测试库，并执行 `cargo test --test schema_contract initializes_the_complete_identity_schema_on_postgres -- --ignored` 及受影响的存储或路由集成测试。SQLite 与 mock 测试只能补充验证，不能证明 PostgreSQL 部署可用；不得使用运行中或生产数据库作为测试库。
- 自动化测试主要验证本地转换和 mock upstream；真实 Codex、Claude Code、上游服务和 Docker 部署需独立联调，不得将前者表述为后者已验收。

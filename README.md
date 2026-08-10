# provider-relay

`provider-relay` 是一个 Rust/Axum 协议桥接网关，向客户端提供 OpenAI Chat Completions、OpenAI Responses 和 Anthropic Messages 入口，并通过 Interface 配置把客户端模型名映射到上游 Provider 模型。

## 环境要求

- Rust 工具链和 Cargo。
- Bun，用于安装、检查、测试和构建 `web/` 管理台。

前端依赖必须按锁文件安装：

```text
cd web
bun install --frozen-lockfile
```

Cargo 构建不会安装前端依赖。`web/node_modules` 不存在时，`build.rs` 会直接失败并给出上述安装命令。Rust 构建会依次检查前端格式、运行 ESLint，并执行类型检查和 Vite 构建；仅在 Docker 的分层 Rust 构建等已有独立前端产物的流程中使用 `SKIP_FRONTEND_BUILD=1`。

## 运行

在仓库根目录运行：

```text
cargo run
```

服务固定使用 `data/relay.db`，首次运行会创建 `data/`。管理台和 API 默认监听 `0.0.0.0:18080`。

运行时读取以下环境变量：

| 变量 | 默认值 | 用途 |
| --- | --- | --- |
| `LISTEN_PORT` | `18080` | HTTP 监听端口 |
| `ADMIN_TOKEN` | 未设置 | `/api/*` 管理接口的 Bearer 或 `x-api-key` 凭据 |
| `MODEL_PRICES_PATH` | `data/model_prices.json` | 本地模型价格文件；文件不存在时不计算成本 |
| `RUST_LOG` | `provider_relay=info,tower_http=info` | tracing 日志过滤规则 |

`ADMIN_TOKEN` 未设置或为空时，管理 API 会以兼容模式开放。该模式以及当前的单一管理令牌只适用于本机或受控网络；它们不构成用户、角色、登录、会话或多租户系统。设置 `ADMIN_TOKEN` 后，管理台请求必须携带相同令牌。管理台从浏览器本地存储键 `provider-relay-admin-token` 读取该值。

## Interface Token

每个 Interface 拥有独立 token 和完整模型映射集合。客户端访问 `/v1/chat/completions`、`/v1/responses`、`/v1/messages`、`/v1/models` 或 `/models` 时必须携带 Interface token；服务先定位 Interface，再按该 Interface 的客户端模型名解析 Provider 和上游模型。Provider token 不能替代 Interface token。

两种请求头均受支持：

```text
Authorization: Bearer <interface-token>
```

```text
x-api-key: <interface-token>
```

例如：

```text
curl http://127.0.0.1:18080/v1/models -H "Authorization: Bearer <interface-token>"
```

## 验证

Rust 代码修改后执行：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

前端在 `web/` 目录执行：

```text
bun test
bun run format:check
bun run lint
bun run typecheck
bun run build
```

提交前在仓库根目录执行：

```text
git diff --check
```

自动化测试和本地构建不代表真实供应商、Codex、Claude Code、浏览器交互或 Docker 镜像已经完成端到端验收。

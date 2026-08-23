# prelay-server

`prelay-server` 是 Rust/Axum 协议桥接服务。它按身份隔离配置、加密保存供应商密钥，并将 AI 工具请求转发到上游 Provider。

管理 API DTO、稳定错误码和 Bruno 协议集合由 `prelay-protocol` 唯一维护。服务端通过 [`crates/protocol`](crates/protocol/) submodule 使用该仓；当前 checkout 中的请求路径、鉴权方式和示例见 [协议集合](crates/protocol/docs/protocol/)。本仓只记录运行、部署和安全边界。

## 运行

首次从 Git 获取源码后初始化协议子模块：

```text
git submodule update --init --recursive
```

服务端必须配置 `PRELAY_MASTER_KEY`。它是 Base64 编码的 32 字节密钥，用于 AES-256-GCM 加密数据库中的 Provider API Key；密钥缺失、格式非法或长度不正确时服务拒绝启动。

生成一个密钥：

```text
openssl rand -base64 32
```

Windows PowerShell：

```powershell
[Convert]::ToBase64String([System.Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
```

本地 `.env` 会在启动时自动加载；已有进程环境变量优先。开发运行：

```powershell
$env:PRELAY_MASTER_KEY = "<Base64-encoded-32-byte-key>"
cargo run
```

服务使用 `data/relay.db`，首次运行会创建 `data/`，默认监听 `0.0.0.0:18080`。`LISTEN_PORT` 可覆盖端口，`RUST_LOG` 控制日志过滤。模型价格可放入 `config/model_prices.json`，或通过 `MODEL_PRICES_PATH` 指定其他文件。

启动时及之后每 24 小时会删除连续 90 天未活动身份及其所有配置、会话和日志。旧版没有身份归属的数据库会在首次启动时被直接丢弃，不迁移或保留旧密钥。

## 部署

Docker Compose 文件位于 `deploy/`。复制环境模板，填入固定主密钥后启动：

```powershell
Copy-Item deploy/.env.example deploy/.env
# Edit deploy/.env and set PRELAY_MASTER_KEY.
docker compose -f deploy/docker-compose.yml up -d
```

Compose 固定拉取 `ghcr.io/zoranner/prelay-server:0.1.0`，使用 `data/` 作为数据库卷，并以只读方式挂载 `app/config/`。模型价格是可选配置；可复制 `config/model_prices.example.json` 为 `app/config/model_prices.json` 后调整内容。

## 服务边界

- 正式调用入口固定为 `/v1/models`、`/v1/responses`、`/v1/chat/completions` 和 `/v1/messages`，由 Endpoint Token 授权。
- 管理 API 位于 `/api/*`，由设备凭据授权；`POST /api/identities` 是唯一匿名入口。
- Provider API Key 只以 `PRELAY_MASTER_KEY` 加密后保存。数据库、`.env`、主密钥、设备凭据、Endpoint Token 和真实 Provider API Key 不得提交或记录到日志。
- 服务自身不提供 TLS。暴露到非受信网络时，部署环境必须提供 TLS 与网络访问控制。
- 通用 `/proxy` 入口已移除，不应恢复。

## 上游故障转移

上游故障转移属于服务端部署策略，不向桌面客户端或最终用户开放。以下变量在启动时读取，修改后需要重启服务：

- `PRELAY_UPSTREAM_TIMEOUT_SECS`：每次上游请求的超时秒数，默认 `300`，必须大于 `0`。
- `PRELAY_UPSTREAM_MAX_RETRIES`：同一候选上游的额外重试次数，默认 `0`。
- `PRELAY_UPSTREAM_RETRY_BACKOFF_MS`：重试前等待的毫秒数，默认 `250`，允许为 `0`。
- `PRELAY_UPSTREAM_MAX_CANDIDATES`：单次请求最多尝试的候选上游数，默认不限制，必须大于 `0`。

只有连接失败、超时、HTTP `408`、`429` 和 `5xx` 会触发重试或按接入点候选顺序切换。认证、权限、模型或请求参数错误会直接返回。流式请求仅在上游尚未返回成功响应前切换候选；数据开始向客户端输出后不会重新请求上游。

## 验证

Rust 代码修改后在仓库根目录执行：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
```

自动化测试和本地构建不代表真实供应商或 Docker 镜像已经完成端到端验收。

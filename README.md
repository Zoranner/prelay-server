# provider-relay

`provider-relay` 是 Rust/Axum 协议桥接服务。桌面客户端负责保存用户的供应商配置和设备凭据；服务端按身份隔离配置、加密保存供应商密钥，并把 AI 工具的请求桥接到上游 Provider。

## 服务端运行

服务端必须设置 `PROVIDER_RELAY_MASTER_KEY`：它是 Base64 编码的 32 字节密钥，用于 AES-256-GCM 加密数据库中的 Provider API Key。密钥缺失、格式非法或长度不正确时服务拒绝启动。

生成一个密钥：

```text
openssl rand -base64 32
```

Windows PowerShell 也可生成：

```powershell
[Convert]::ToBase64String([System.Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
```

本地开发运行：

```text
$env:PROVIDER_RELAY_MASTER_KEY = "<Base64-encoded-32-byte-key>"
cargo run -p provider-relay-server
```

服务固定使用 `data/relay.db`，首次运行会创建 `data/`，默认监听 `0.0.0.0:18080`。可通过 `LISTEN_PORT` 覆盖端口；`RUST_LOG` 控制日志过滤。启动时及之后每 24 小时会删除连续 90 天未活动身份及其所有配置、会话和日志。旧版没有身份归属的数据库会在首次启动时被直接丢弃，不迁移或保留旧密钥。

Docker Compose 同样要求在环境中提供 `PROVIDER_RELAY_MASTER_KEY`：

```text
PROVIDER_RELAY_MASTER_KEY=<Base64-encoded-32-byte-key> docker compose -f docker/docker-compose.yml up -d --build
```

## 客户端与协议入口

员工在 Windows 桌面客户端中注册当前机器和登录账户，并管理 Provider、Interface 及 Interface Token。设备凭据只保存在 Windows Credential Manager；服务端只保存其哈希。客户端管理 API 位于 `/api/*`，不能用浏览器网页替代。

AI 工具将继续使用以下协议入口：

- `/v1/models`
- `/v1/responses`
- `/v1/chat/completions`
- `/v1/messages`

这些入口要求对应的 Interface Token。不要使用 `/proxy`；该通用入口已移除。

## 验证

Rust 代码修改后执行：

```text
cargo fmt --all
cargo clippy -p provider-relay-server --all-targets --all-features -- -D warnings
cargo test -p provider-relay-server --all-targets --all-features
```

提交前在仓库根目录执行：

```text
git diff --check
```

自动化测试和本地构建不代表真实供应商或 Docker 镜像已经完成端到端验收。

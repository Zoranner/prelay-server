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

```powershell
Set-Location server
$env:PROVIDER_RELAY_MASTER_KEY = "<Base64-encoded-32-byte-key>"
cargo run
```

服务固定使用 `server/data/relay.db`，首次运行会创建 `server/data/`，默认监听 `0.0.0.0:18080`。可通过 `LISTEN_PORT` 覆盖端口；`RUST_LOG` 控制日志过滤。模型价格可放入 `server/config/model_prices.json`，或通过 `MODEL_PRICES_PATH` 指定其他文件。启动时及之后每 24 小时会删除连续 90 天未活动身份及其所有配置、会话和日志。旧版没有身份归属的数据库会在首次启动时被直接丢弃，不迁移或保留旧密钥。

Docker Compose 的部署文件位于 `server/deploy/`。先复制环境模板、填入固定的主密钥，再启动：

```powershell
Copy-Item server/deploy/.env.example server/deploy/.env
# Edit server/deploy/.env and replace PROVIDER_RELAY_MASTER_KEY.
docker compose --env-file server/deploy/.env -f server/deploy/docker-compose.yml up -d --build
```

Compose 使用 `server/data/` 作为数据库卷、以只读方式挂载 `server/config/`，并沿用 `server/Dockerfile` 构建镜像。模型价格为可选配置，可复制 `server/config/model_prices.example.json` 为 `server/config/model_prices.json` 后再调整内容。

## 客户端与协议入口

员工在 Windows 桌面客户端中注册当前机器和登录账户，并管理 Provider、Interface 及 Interface Token。客户端用操作系统 CSPRNG 生成设备凭据，将其写入应用数据目录的 `device-credential.json`；记录只有 `current` 和可选的 `pending` 两个字段。服务端只保存凭据哈希，Nuxt 运行时绝不接收设备凭据或 Provider API Key。

首次注册先写入本地凭据文件，再提交 `machine_id`、`account_sid` 和凭据。同一机器和账户使用同一凭据重复注册会返回已有身份，支持网络中断后的确认重试。轮换先把新凭据保存为 `pending`，使用旧凭据提交轮换；调用或响应中断后，客户端优先尝试 `pending`，认证失败再回退 `current`。本地凭据文件不使用 Windows Credential Manager、系统 Keychain 或加密 vault，适用于内网桌面客户端的本地使用边界。

客户端位于 `client/`，采用 Tauri 2、Nuxt 4 和 Tailwind 4。安装前端依赖与运行开发环境：

```text
cd client
bun install --frozen-lockfile
bun run tauri dev
```

AI 工具将继续使用以下协议入口：

- `/v1/models`
- `/v1/responses`
- `/v1/chat/completions`
- `/v1/messages`

这些入口要求对应的 Interface Token。不要使用 `/proxy`；该通用入口已移除。

管理 API 的请求样例和 Bruno 环境模板位于 [`docs/protocol/`](docs/protocol/README.md)。模板只含占位值，不能写入真实设备凭据、Interface Token 或 Provider API Key。

## 验证

服务端 Rust 代码修改后在 `server/` 执行：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

共享协议与客户端原生代码分别在 `crates/protocol/` 和 `client/src-tauri/` 目录执行各自的 Cargo 检查；根目录不再是 Cargo workspace。

提交前在仓库根目录执行：

```text
git diff --check
```

自动化测试和本地构建不代表真实供应商或 Docker 镜像已经完成端到端验收。

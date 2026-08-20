# prelay-server

`prelay-server` 是 Rust/Axum 协议桥接服务。它按身份隔离配置、加密保存供应商密钥，并把 AI 工具请求桥接到上游 Provider。管理 API 的 DTO 与稳定错误码来自 Git 子模块 `crates/protocol` 中的 `prelay-protocol`。

## 服务端运行

首次从 Git 获取源码后，先初始化协议子模块：

```text
git submodule update --init --recursive
```

服务端必须设置 `PRELAY_MASTER_KEY`：它是 Base64 编码的 32 字节密钥，用于 AES-256-GCM 加密数据库中的 Provider API Key。密钥缺失、格式非法或长度不正确时服务拒绝启动。

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
$env:PRELAY_MASTER_KEY = "<Base64-encoded-32-byte-key>"
cargo run
```

服务固定使用 `data/relay.db`，首次运行会创建 `data/`，默认监听 `0.0.0.0:18080`。可通过 `LISTEN_PORT` 覆盖端口；`RUST_LOG` 控制日志过滤。模型价格可放入 `config/model_prices.json`，或通过 `MODEL_PRICES_PATH` 指定其他文件。启动时及之后每 24 小时会删除连续 90 天未活动身份及其所有配置、会话和日志。旧版没有身份归属的数据库会在首次启动时被直接丢弃，不迁移或保留旧密钥。

Docker Compose 的部署文件位于 `deploy/`。先复制环境模板、填入固定的主密钥，再启动：

```powershell
Copy-Item deploy/.env.example deploy/.env
# Edit deploy/.env and set PRELAY_MASTER_KEY.
docker compose -f deploy/docker-compose.yml up -d
```

Compose 固定拉取 `ghcr.io/zoranner/prelay-server:0.1.0`，使用 `data/` 作为数据库卷，并以只读方式挂载 `config/`。模型价格为可选配置，可复制 `config/model_prices.example.json` 为 `config/model_prices.json` 后再调整内容。

## AI 工具接口

AI 工具使用以下协议入口：

- `/v1/models`
- `/v1/responses`
- `/v1/chat/completions`
- `/v1/messages`

这些入口要求对应的 Interface Token。不要使用 `/proxy`；该通用入口已移除。

## 验证

Rust 代码修改后在仓库根目录执行：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

`prelay-protocol` 由 `crates/protocol` 子模块独立维护；本仓不包含客户端源码。

提交前执行：

```text
git diff --check
```

自动化测试和本地构建不代表真实供应商或 Docker 镜像已经完成端到端验收。

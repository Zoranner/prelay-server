# provider-relay

`provider-relay` 是 Rust/Axum 协议桥接服务。它为 AI 工具保留 OpenAI Responses、OpenAI Chat Completions 与 Anthropic Messages 的统一入口，并通过 Interface 配置把客户端模型名映射到上游 Provider 模型。

## 当前迁移状态

仓库正在将网页管理台迁移到桌面客户端。此提交只完成服务端目录拆分：网页静态入口、全局管理令牌和通用转发入口均已移除，而桌面客户端和身份认证管理 API 尚未就绪。因此当前版本是中间迁移版本，不能作为可部署版本。

## 服务端开发

服务端在 `server/` 中：

```text
cargo run -p provider-relay-server
```

服务固定使用 `data/relay.db`，首次运行会创建 `data/`，默认监听 `0.0.0.0:18080`。可通过 `LISTEN_PORT` 覆盖端口；`RUST_LOG` 控制日志过滤。

AI 工具将继续使用以下协议入口：

- `/v1/models`
- `/v1/responses`
- `/v1/chat/completions`
- `/v1/messages`

这些入口要求 Interface Token。身份认证的管理 API 将在后续迁移任务中提供。

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

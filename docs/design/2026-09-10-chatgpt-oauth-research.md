# ChatGPT OAuth 参考实现调研

> 状态：调研材料，不代表 Prelay 已决定实施。
> 日期：2026-09-10

## 目的

记录 `prelay-server/.refs` 中参考项目对 OpenAI 官方账号登录的实现方式，说明它与普通 OpenAI API Provider 的区别，并为后续是否支持提供判断依据。

## 结论

参考项目 `cc-switch` 支持的不是 OpenAI API OAuth，而是 **ChatGPT 账号的 Codex OAuth**。用户通过 OpenAI 的 Codex 设备授权页面登录 ChatGPT，客户端取得 OAuth 凭据，再访问 ChatGPT 的 Codex 后端接口。这个能力被放在 Claude Provider 中，以反向代理形式让 Claude Code 使用 Codex 服务。

因此，“官方账号登录”在技术上可以作为独立的 `Codex OAuth` 凭据类型研究，但不能复用普通 API Key Provider 的语义，也不能把它描述成 OpenAI API 官方第三方 OAuth 集成。

## 参考项目证据

主要来源：

- `prelay-server/.refs/cc-switch/docs/user-manual/en/2-providers/2.1-add.md`
- 章节：`Codex OAuth Reverse Proxy (Claude Provider)`

参考项目明确记录了以下事实：

- 登录入口为 `https://auth.openai.com/codex/device`。
- 登录方式是 Device Code Flow，展示验证码并轮询授权结果。
- 授权完成后，账号出现在本地 OAuth Auth Center。
- Refresh token 保存在本地数据目录，不导出、不上传。
- 请求目标为 `https://chatgpt.com/backend-api/codex`。
- 协议固定为 `openai_responses`。
- 支持多个 ChatGPT 账号、账号选择、移除和 token 自动刷新。
- 模型列表可以从 Codex 后端动态查询。
- 参考项目自己把这条链路称为 reverse-engineered OAuth flow，并提示服务条款、账号风控和长期可用性风险。

## 实际链路

```text
ChatGPT 账号
  -> auth.openai.com/codex/device
  -> Device Code 授权
  -> OAuth access token / refresh token
  -> chatgpt.com/backend-api/codex
  -> Codex Responses 协议
  -> 反向代理
  -> Claude Code
```

参考项目的关键点是“产品入口”和“上游能力”分离：登录入口属于 OAuth Auth Center，Provider 卡片只选择已经登录的账号；Provider 启用后才把请求路由到 Codex 后端。

## 与普通 OpenAI API Provider 的区别

| 项目 | OpenAI API Provider | ChatGPT Codex OAuth |
| --- | --- | --- |
| 用户凭据 | API key | ChatGPT 账号授权产生的 OAuth 凭据 |
| 上游 | OpenAI API | ChatGPT Codex backend |
| 主要协议 | API 的 Responses / Chat Completions | Codex Responses |
| 计费或额度 | API 项目额度 | ChatGPT 账号的 Codex 配额 |
| 凭据生命周期 | API key 撤销或轮换 | access token 过期、refresh、撤销 |
| 账号风险 | API key 权限和项目限制 | 账号自动化检测、授权流程变化 |
| 稳定性 | 面向 API 的公开契约 | 依赖 Codex 登录和后端实现 |
| Provider 建模 | `api_key` | 应独立建模为 `codex_oauth` |

## 对 Prelay 的含义

如果未来支持，最小边界应是：

- 在协议层把 `codex_oauth` 与 `api_key` 分开。
- 服务端独立处理 device authorization、token 加密保存、刷新和撤销。
- Provider 绑定具体 ChatGPT 账号，而不是只绑定 Provider 名称。
- 依据凭据类型选择 OpenAI API 或 ChatGPT Codex upstream。
- 对外仍使用 Prelay 已有的 Endpoint Token 和 `/v1/responses` 等入口。
- 客户端只通过 Tauri 原生命令发起授权、选择账号和显示状态。
- 不把 refresh token、access token、设备凭据写入日志、文档、测试夹具或客户端持久化状态。
- 明确标记为实验性能力，并保留授权失败、token 过期、刷新失败、上游变化等状态。

这里的“对外仍使用现有入口”只是协议适配层判断，不表示当前已经授权开发或已经确认上游可长期使用。

## 不应直接照搬的部分

参考项目运行在本地桌面环境，refresh token 只保存在本地；Prelay 的服务端负责多设备、身份隔离和供应商凭据加密保存。因此不能直接复制其本地文件存储模型。

参考项目把 Codex OAuth 放进 Claude Provider，是为了让 Claude Code 消费 Codex 服务。Prelay 需要重新判断 Provider、Endpoint、设备身份和账号绑定之间的关系，不能仅复制一个前端卡片。

参考项目文档给出了可运行的产品路径，但同时明确承认它依赖逆向 OAuth 流程。它不能单独证明该流程属于 OpenAI 面向第三方服务的稳定、公开 API 授权，也不能证明代理 ChatGPT Codex 配额没有服务条款或账号风控问题。

## 是否值得做

当前更适合把它作为独立调研项保留，不并入普通 OpenAI Provider。只有在确认以下问题后，才有理由进入实现评估：

- OpenAI 当前是否继续提供该设备授权入口。
- 授权结果和 Codex backend 的使用范围是否发生变化。
- Prelay 是否接受依赖非稳定公开契约带来的维护和账号风险。
- 服务端保存 ChatGPT OAuth 长期凭据是否符合产品的部署和责任边界。
- 是否需要限制为用户自托管、单设备或明确的实验性功能。

## 当前决定

本文件只完成参考实现记录和边界分析。当前不修改协议、服务端、客户端、部署配置或 Provider 数据模型，也不把 `Codex OAuth` 标记为已支持能力。

## 参考文件

- `prelay-server/.refs/cc-switch/docs/user-manual/en/2-providers/2.1-add.md:372-505`
- `prelay-server/.refs/codex-bridge/README.md:24-26, 65-89`

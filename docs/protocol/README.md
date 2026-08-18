# Provider Relay 协议请求

本目录是 Provider Relay 的 Bruno 请求集合。`environments/template.bru` 只包含本地示例与占位符；复制为个人环境后填写实际地址和凭据，不能将该个人环境提交到版本库。

设备注册请求为 `POST /api/identities`，发送 `machine_id`、`account_sid` 与客户端生成的 `credential`。凭据是操作系统 CSPRNG 生成的 32 字节 URL-safe Base64 值。服务端首次注册返回 `201 Created` 与 `{ "identity_id": "<identity-id>", "created": true }`；同一 `machine_id + account_sid` 且凭据相同的重试返回 `200 OK` 与 `{ "identity_id": "<identity-id>", "created": false }`。稳定键已存在而凭据不同会被拒绝。

除注册外的管理请求均使用 `Authorization: Bearer {{device_credential}}`。轮换请求为 `POST /api/identity/credential/rotate`，正文只包含 `new_credential`，成功响应为 `{ "rotated": true }`，不会返回凭据。桌面客户端的本地文件使用 `{ "current": "...", "pending": "..." }` 管理轮换恢复：先写入 `pending`，再以 `current` 认证；`pending` 认证失败则恢复 `current`，网络中断则保留两个值下次重试。

本地文件位于客户端应用数据目录的 `Provider Relay/device-credential.json`。它不使用 Windows Credential Manager、系统 Keychain 或加密 vault，适用于内网桌面客户端的本地使用边界。Nuxt 运行时、管理 API 响应和 Bruno 集合都不应接收或保存真实 Provider API Key。

AI 工具使用根路径下的 `/v1/models`、`/v1/responses`、`/v1/chat/completions` 和 `/v1/messages`，通过 Interface Token 认证；`/proxy` 已移除。

# provider-relay 身份隔离与桌面客户端设计

## 目标

系统面向无法直接访问公网的内网员工提供大模型协议接入。服务端负责协议转换、模型路由、供应商调用和请求观测；员工通过桌面客户端管理自己的供应商、密钥、模型和接口，不再使用网页管理台。

服务端按客户端身份隔离全部配置和统计。员工 AI 工具直接调用服务端的 `/v1/*` 协议入口，桌面客户端不提供本地协议服务。

## 边界

### 服务端职责

- 提供 OpenAI Responses、OpenAI Chat Completions、Anthropic Messages 和模型列表协议入口。
- 根据 Interface Token 解析同一身份下的模型映射，执行既有协议转换并调用供应商 API。
- 加密保存供应商 API Key，保存会话和请求观测数据。
- 为桌面客户端提供受身份限制的配置、连通性测试、统计和诊断 API。
- 清理长期失活身份及其关联数据。

### 桌面客户端职责

- 采用 Tauri 2、Nuxt 4 和 Tailwind 4。
- 替代现有管理台的全部功能：供应商与密钥、模型、Interface、接口令牌复制与重置、连通性测试、请求统计、明细和错误诊断。
- 首次启动注册身份，并将服务端签发的凭据保存在 Windows 凭据管理器。
- 登录后自动启动并驻留系统托盘，用户按需打开管理窗口。

### 非目标

- 不提供网页管理台、全局管理令牌、管理员、角色、人工审批、恢复码或跨设备身份迁移。
- 不提供本地协议代理，不要求 AI 工具连接 `127.0.0.1`。
- 不提供 `/proxy` 透明转发入口，不允许协议请求绕过 Interface Token 与模型解析。
- HTTPS、域名、证书、端口和内网网络策略由部署环境负责，不属于应用协议契约。

## 身份与认证

### 身份键

`identities` 表表示一台电脑上一个 Windows 登录账户的身份。身份的稳定键为 `machine_id + account_sid`：

- `machine_id` 区分电脑。
- `account_sid` 区分同一电脑上的 Windows 登录账户；域账户和本地账户均使用 SID，不使用可变且不唯一的用户名。
- 同一电脑、同一 Windows 账户重装客户端后，稳定键不变，继续对应原有身份和配置。
- 更换电脑、重装 Windows 或 Windows 账户 SID 变化时，创建新身份并重新配置。

用户名只可作为客户端展示信息，不参与身份定位或授权。`machine_id` 和 `account_sid` 都是身份定位信息，不能单独用于认证。

### 设备凭据

首次运行且 Windows 凭据管理器中不存在设备凭据时，桌面客户端提交当前 `machine_id` 与 `account_sid`。服务端只在该稳定键尚未登记时创建身份，并签发高随机设备凭据。设备凭据明文仅在注册响应中返回一次，客户端立即保存到 Windows 凭据管理器；服务端仅保存其哈希。稳定键已存在时，注册请求不得重新签发凭据。

后续桌面客户端请求必须携带设备凭据。服务端从凭据得到当前 `identity_id`，客户端不得提交、选择或覆盖该值。重装客户端时，Windows 凭据管理器中保留的凭据继续证明原身份；若该凭据被清除，不能仅凭机器与账户信息重新签发凭据。

设备凭据可以由客户端主动轮换。轮换后旧凭据立即失效。系统不提供管理员找回、身份接管或人工重置路径。

## 数据归属

`identities` 保存以下事实：

- `id`：服务端身份 ID。
- `machine_id`：机器标识。
- `account_sid`：Windows 账户 SID。
- `credential_hash`：当前设备凭据哈希。
- `created_at`：身份创建时间。
- `last_active_at`：最后活动时间。

`machine_id + account_sid` 必须唯一。Provider、Provider 模型、Interface、Interface 模型、会话、请求日志和模型别名均直接或经父资源归属同一个 `identity_id`。查询、创建、更新、删除和统计聚合都必须在服务端按当前认证身份限定；对象 ID 猜测、跨身份同名模型和跨身份 Interface Token 都不能越过该限制。

Provider API Key 由服务端使用部署提供的主密钥加密后保存。主密钥不写入数据库、客户端或版本库。读取 Provider 配置时只返回脱敏值；服务端仅在调用对应供应商时于内存中解密。删除 Provider 或身份时，相关密文必须随关联数据在同一事务中删除。

## API 契约

### 桌面客户端 API

唯一匿名管理入口是：

```text
POST /api/identities
```

该入口接收首次注册所需的机器和账户定位信息，返回身份 ID 与一次性设备凭据。已认证客户端的管理与统计 API 统一携带设备凭据，并只操作当前身份的数据：

```text
GET/POST/PATCH/DELETE /api/providers
POST /api/providers/:id/ping
POST /api/providers/:id/discover-models
POST /api/providers/:id/test-protocol

GET/POST/PATCH/DELETE /api/interfaces
POST /api/interfaces/:id/regenerate-token

GET /api/stats/overview
GET /api/stats/requests
GET /api/stats/models
GET /api/stats/providers
POST /api/identity/credential/rotate
```

Provider 与 Interface 的完整集合原子保存、模型引用校验、连通性测试和统计诊断保持既有语义，但所有资源必须属于当前身份。接口不接受客户端传入的 `identity_id`；服务端从设备凭据确定归属。

### AI 工具 API

AI 工具只使用根路径下的正式协议入口：

```text
/v1/models
/v1/responses
/v1/chat/completions
/v1/messages
```

每个请求必须携带 Interface Token。服务端先定位该 Interface 及其 `identity_id`，再只在该身份的 Provider 和模型映射中解析模型并调用供应商。Interface Token 是 AI 工具的协议访问凭据，不是桌面客户端身份凭据，不能访问 `/api/*`。

## 生命周期

成功的设备凭据管理请求、统计请求和通过该身份 Interface Token 完成的协议请求都会刷新 `last_active_at`。服务端对连续 90 天未活动的身份直接删除，不发送提醒。

删除身份时必须级联删除其 Provider、密钥密文、模型、Interface、Interface Token、会话、请求日志和模型别名；删除过程不保留恢复记录或归档数据。删除 Interface、Provider 或模型时也必须保留既有的引用清理和事务一致性。

## 迁移

这是不兼容迁移。旧的无身份 Provider、Interface、模型映射、会话和统计数据不自动分配给任何新身份；升级时清除这些旧配置数据。旧网页静态管理台、`ADMIN_TOKEN` 和 `/proxy` 一并移除。

协议转换、上游适配、流式处理、会话语义、模型能力判断与统计计算保留。桌面客户端取代网页管理台成为唯一配置和观测界面。

## 验收

- 首次启动客户端自动创建同机同账户身份，并将设备凭据保存在 Windows 凭据管理器；已有稳定键不能通过注册接口重新签发凭据。
- 同机同账户重装客户端且凭据仍存在时，可继续访问同一身份的配置和统计；不同电脑或不同 Windows SID 不能共享身份。
- 设备凭据只能访问所属身份的资源；猜测资源 ID、同名模型和其他身份的 Interface 都不能跨越隔离边界。
- Provider 密钥读取响应不含明文，服务端仍能用密文完成上游调用。
- `/v1/*` 只接受 Interface Token，并只从其身份范围内解析模型；`/proxy` 不存在。
- 桌面客户端覆盖现有管理台功能，且具备用户登录后自启与系统托盘驻留行为。
- 90 天无活动身份及其全部关联数据被直接删除。

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
- 首次启动由客户端生成设备凭据，并保存在应用数据目录的本地凭据文件中。
- 登录后自动启动并驻留系统托盘，用户按需打开管理窗口。

### 非目标

- 不提供网页管理台、全局管理令牌、管理员、角色、人工审批、恢复码或跨设备身份迁移。
- 不提供本地协议代理，不要求 AI 工具连接 `127.0.0.1`。
- 不提供 `/proxy` 透明转发入口，不允许协议请求绕过 Interface Token 与模型解析。
- HTTPS、域名、证书、端口和内网网络策略由部署环境负责，不属于应用协议契约。

## 仓库结构

仓库是 Cargo workspace，成员为 `server`、`client/src-tauri` 和 `crates/protocol`。服务端、桌面客户端与共享管理协议显式分离：

```text
provider-relay/
├─ Cargo.toml
├─ server/
│  ├─ Cargo.toml
│  ├─ Dockerfile
│  └─ src/
│     ├─ main.rs
│     ├─ app.rs
│     ├─ identity/
│     ├─ storage/
│     ├─ routes/
│     │  ├─ management/
│     │  └─ v1/
│     ├─ bridge/
│     ├─ providers/
│     └─ observability/
├─ client/
│  ├─ app/
│  │  ├─ pages/
│  │  ├─ components/
│  │  ├─ composables/
│  │  ├─ stores/
│  │  └─ utils/
│  └─ src-tauri/
│     └─ src/
│        ├─ commands/
│        ├─ api_client.rs
│        ├─ identity.rs
│        ├─ credential_store.rs
│        ├─ autostart.rs
│        └─ tray.rs
├─ crates/
│  └─ protocol/
│     └─ src/
│        ├─ identity.rs
│        ├─ providers.rs
│        ├─ interfaces.rs
│        ├─ stats.rs
│        ├─ error.rs
│        └─ lib.rs
├─ docs/
│  └─ protocol/
│     ├─ management/
│     ├─ v1/
│     ├─ environments/
│     └─ bruno.json
└─ docker/
```

`server/src/main.rs` 只负责启动，`app.rs` 负责运行时状态和路由装配。`identity/` 管理注册、凭据认证、轮换和失活清理；`storage/` 管理 SQLite 事务与密钥密文；`routes/management/` 提供桌面客户端的 `/api/*`；`routes/v1/` 提供 AI 工具的 `/v1/*`。

现有协议桥接内核迁入 `server/src/bridge/`，上游协议适配迁入 `server/src/providers/`，不因目录迁移改变其协议职责。请求记录与统计聚合位于 `server/src/observability/`。

Nuxt 页面只通过 Tauri command 调用客户端原生层。`client/src-tauri/src/api_client.rs` 是唯一持有服务端请求与设备凭据的位置；凭据不暴露给 Nuxt 运行时。`crates/protocol` 仅定义客户端与服务端之间的管理 API 请求、响应和稳定错误码，不包含 HTTP 路由、SQLite、密钥加密、上游供应商适配或协议桥接实现。`docs/protocol/` 保存协议说明和 Bruno 验证集合及无密钥环境模板。

## 身份与认证

### 身份键

`identities` 表表示一台电脑上一个 Windows 登录账户的身份。身份的稳定键为 `machine_id + account_sid`：

- `machine_id` 区分电脑。
- `account_sid` 区分同一电脑上的 Windows 登录账户；域账户和本地账户均使用 SID，不使用可变且不唯一的用户名。
- 同一电脑、同一 Windows 账户重装客户端后，稳定键不变，继续对应原有身份和配置。
- 更换电脑、重装 Windows 或 Windows 账户 SID 变化时，创建新身份并重新配置。

用户名只可作为客户端展示信息，不参与身份定位或授权。`machine_id` 和 `account_sid` 都是身份定位信息，不能单独用于认证。

### 设备凭据

设备凭据是客户端生成的高随机值。客户端将其存入应用数据目录的本地凭据文件，并通过临时文件写入后原子替换；该文件不使用 Windows Credential Manager、系统 Keychain 或加密 vault。安全边界是内网本地使用，防止意外明文散落，不防同一登录账户上的恶意程序读取本地应用数据。

首次运行时，客户端先生成并原子保存凭据，再提交 `machine_id`、`account_sid` 和凭据。服务端仅保存凭据哈希：稳定键尚未登记时创建身份；稳定键已登记且凭据哈希相同则返回已有身份，使客户端可安全重试；稳定键已登记但凭据不同则拒绝，不重新签发或覆盖凭据。

后续桌面客户端请求携带本地凭据，服务端从凭据得到当前 `identity_id`，客户端不得提交、选择或覆盖该值。凭据文件被清除后，不能仅凭机器与账户信息重新签发凭据。

设备凭据可以主动轮换。客户端先原子保存包含旧凭据和待生效新凭据的本地记录，再使用旧凭据提交新凭据。服务端原子替换哈希，新凭据立即生效。轮换调用或响应中断时，客户端优先使用待生效新凭据；新凭据未认证时回退旧凭据并清除待生效值；网络错误时保留两个值等待重试。轮换完成后，客户端原子保存仅含新凭据的记录。系统不提供管理员找回、身份接管或人工重置路径。

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

该入口接收首次注册所需的机器、账户定位信息和客户端生成的设备凭据，返回身份确认结果。已认证客户端的管理与统计 API 统一携带设备凭据，并只操作当前身份的数据：

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

- 首次启动客户端先原子保存本地设备凭据，再创建同机同账户身份；本地凭据不依赖 Windows Credential Manager 或加密 vault。
- 注册请求响应丢失后，客户端以同一凭据重试可继续访问原身份；已有稳定键不能使用不同凭据重新签发或接管身份。
- 轮换请求或响应中断后，客户端可在待生效新凭据和旧凭据之间恢复到服务端的实际状态；不同电脑或不同 Windows SID 不能共享身份。
- 设备凭据只能访问所属身份的资源；猜测资源 ID、同名模型和其他身份的 Interface 都不能跨越隔离边界。
- Provider 密钥读取响应不含明文，服务端仍能用密文完成上游调用。
- `/v1/*` 只接受 Interface Token，并只从其身份范围内解析模型；`/proxy` 不存在。
- 桌面客户端覆盖现有管理台功能，且具备用户登录后自启与系统托盘驻留行为。
- 90 天无活动身份及其全部关联数据被直接删除。

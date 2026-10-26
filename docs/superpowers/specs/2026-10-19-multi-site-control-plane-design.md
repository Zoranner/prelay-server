# Prelay 多站点控制面与网关架构设计

## 架构结论

采用控制面/数据面分离：

- `prelay-control`：全局身份、Provider/Endpoint 元数据、模型目录、Endpoint Token 签发、站点策略、配置分发、统计和审计。
- `prelay-gateway`：LLM 代理、上游调用、本地活动、outbox、配置缓存和 Provider 密钥信封。
- 1 个控制面 + 3 个站点网关（总部、分公司、公网）。
- 4 个独立 PostgreSQL：控制面 1 个，三个网关各 1 个。
- 控制面放在总部管理区，通过私网/VPN 连接三个网关；公网网关是唯一面向互联网的 LLM 入口。
- 客户端只访问本站网关；管理 API 由网关边缘转发到控制面。
- 客户端使用站点 profiles，不引入本地 AI 转发代理。

Provider 只配置一次，由控制面的全局 Provider 目录完成，不需要在三个站点重复录入。

## 部署拓扑

```text
                         ┌──────────────────────────────────────┐
                         │ 总部管理区（不对公网开放）            │
                         │ prelay-control (2+ replicas)         │
                         │ control PostgreSQL (HA)              │
                         │ KMS / Vault                          │
                         │ OIDC connector                       │
                         └──────────────▲───────────────────────┘
                                        │ 私网 / VPN / mTLS
          ┌─────────────────────────────┼─────────────────────────────┐
          │                             │                             │
 ┌────────┴─────────┐        ┌──────────┴─────────┐        ┌──────────┴─────────┐
 │ 总部网关          │        │ 分公司网关          │        │ 公网网关            │
 │ prelay-gateway-hq│        │ prelay-gateway-br  │        │ prelay-gateway-pub │
 │ 本地 PostgreSQL   │        │ 本地 PostgreSQL     │        │ 本地 PostgreSQL     │
 └────────▲─────────┘        └──────────▲─────────┘        └──────────▲─────────┘
          │                             │                             │
     总部客户端                    分公司客户端                    外网客户端
```

| 组件 | 部署位置 | 域名示例 | 数据库 |
|---|---|---|---|
| `prelay-control` | 总部管理区 | `control.prelay.internal` | 中心 PostgreSQL |
| `prelay-gateway-hq` | 总部内网 | `ai-hq.corp.example` | 总部 PostgreSQL |
| `prelay-gateway-branch` | 分公司内网 | `ai-branch.corp.example` | 分公司 PostgreSQL |
| `prelay-gateway-public` | 公网 DMZ | `ai.example.com` | 公网 PostgreSQL |

控制面不暴露到互联网。分公司和公网网关通过站点到站点 VPN/专线访问控制面；总部网关走私网。控制面不主动连接网关。

## 服务职责

### prelay-control

- 设备身份、设备凭据、OIDC 身份映射。
- 全局 Provider、Endpoint、模型路由和模型目录。
- Provider 密钥主副本和站点信封。
- Endpoint Token 签发、刷新、撤销和审计记录。
- 站点注册、站点策略、配置版本、配置快照和签名。
- 全局用量事实、统计、审计元数据和系统审计日志。
- 管理 API、扩展目录、客户端更新。

控制面不处理 `/v1` 流量，不保存提示词和模型输出。Provider 明文只在创建或轮换时短暂存在于控制面内存，不写日志、不落明文库。

### prelay-gateway

- `/v1/models`、`/v1/responses`、`/v1/chat/completions`、图片生成。
- Endpoint Token 本地验签、模型路由、Provider 候选选择、上游调用和流式转发。
- `/api/*` 管理边缘：通过 mTLS 转发到控制面。
- 本地活动、`activity_contents`、Token/延迟观测。
- 本地 outbox、用量批量推送、断网缓冲和补推。
- 配置缓存、Endpoint 缓存、Provider 密钥信封缓存。
- 本站 Provider 密钥解封；明文只在内存中使用，使用后清零。

网关启动后，只要配置快照、签名公钥、Provider 信封和本地密钥仍在，配置租约和 Endpoint Token 有效期内的已授权请求就可以继续，不依赖控制面实时可用。

### 职责边界

- 控制面不提供 `/v1`。
- 网关的 `/api` 是边缘转发，不是第二套管理事实。
- 网关不保存全局身份和配置的权威副本，只保存可验证的缓存。
- 控制面数据库和每个网关数据库独立迁移，不做 schema 锁步。

## 用户与所有权

产品里没有管理员角色，只有用户。

- Provider 的所有者是创建它的用户。只有所有者能编辑、轮换、删除和设置分享关系。
- Provider 所有者通过 `provider_site_policies` 选择启用站点；没有站点管理员。
- Provider 和 Endpoint 的名称在所有者范围内唯一；不同用户可以重名。
- 其他用户通过 `private`/`selected`/`all` 分享关系只读使用 Provider，不接触密钥。
- Endpoint 的所有者也是创建它的用户。Endpoint 在控制面全局保存一份，三个站点共享同一份模型映射；Endpoint Token 按站点签发。
- Endpoint 当前按设备身份归属；同一人员的多台设备不会自动合并 Endpoint 配置。OIDC 用于人员统计和身份识别，不自动合并设备 Endpoint。
- Provider/Endpoint 不复制到站点；控制面是用户数据的唯一来源，网关只缓存。
- 站点注册、证书轮换是部署动作，不是产品角色；不引入站点管理员。
- `personal`/`team` 都是登录用户可见的聚合口径；个人明细只对自己；不引入全局管理员或审计员角色。
- 审计日志是系统数据，由部署方按基础设施权限访问，不作为产品角色。
- Provider 所有者离职或身份删除时，只能由所有者转移给另一个用户，或者自动禁用；不引入管理员接管。

## Provider 只配置一次

这是本架构对“同一 Provider 配三次不可接受”的直接解决方式。

### 全局 Provider

控制面维护唯一的全局 Provider 记录：

- 名称、类型、Base URL、模型清单、能力声明。
- 所有者、分享关系、启用站点。
- Provider 密钥主副本。

Provider 创建者在控制面创建一次 Provider、填写一次 API Key。三个站点不再各自创建 Provider，也不再各自录入 API Key。创建者就是所有者，其他用户按分享关系使用。

### 密钥分发

1. 控制面用 KMS/Vault 主密钥加密 Provider 密钥，保存到 `provider_secret_master`。
2. 对每个启用该 Provider 的站点，控制面用站点公钥生成一份信封，保存到 `provider_secret_envelopes`。
3. 网关通过配置快照只拿到本站信封。
4. 网关用本地 KMS/TPM 中的站点私钥解封，明文只在内存中使用。

站点之间不共享明文密钥，也不共享同一个站点私钥。某个站点被攻破时，只暴露该站点已启用的 Provider 信封；Provider 密钥轮换时，控制面重新生成主副本和所有启用站点信封，提升配置版本。

### 全局 Endpoint

Endpoint 和模型路由也由控制面统一维护。用户配置一次自己的模型映射，三个站点通过配置快照获得同一份 Endpoint 元数据。站点策略决定哪些模型在该站点可用；不可用时返回明确错误，而不是要求用户在每个站点复制一份配置。

Endpoint Token 仍按站点签发：同一个 Endpoint 在不同站点有各自的令牌，客户端 profiles 自动获取。这样既避免 Endpoint 配置重复，也保持站点隔离。

### Provider 变更

- 新增/修改 Provider：控制面更新主副本和站点信封，提升受影响站点版本。
- 新增站点：控制面为该站点生成已有 Provider 的信封。
- 站点禁用 Provider：删除该站点信封，站点同步后不再解析该 Provider。
- 站点密钥轮换：站点生成新密钥对，控制面重新生成信封，旧公钥在重叠窗口后退休。
- 站点撤销：删除信封、撤销站点证书，租约到期后网关停止新请求。

## 身份与令牌

### 设备身份

设备身份使用 `machine_id + account_sid` 定位。设备凭据由控制面签发，控制面保存哈希，客户端使用 Windows 凭据保护存储。

设备凭据是管理 API 的认证依据。网关的 `/api` 边缘只转发请求，不本地验证设备凭据；控制面是唯一验证方。

### OIDC 人员身份

控制面是 OIDC Relying Party，人员身份使用 `(issuer, subject)`。设备身份和人员身份通过带生效/失效时间的 `identity_subject_bindings` 绑定。

内网客户端使用 OIDC 设备授权流程：

1. 客户端向本站网关发起登录。
2. 网关把请求转发到控制面。
3. 控制面按站点选择 `oidc_providers`，向 IdP 请求 device code。
4. 客户端显示 `user_code` 和验证地址。
5. 用户在本站 OIDC 接入点或手机/外网设备完成登录。
6. 控制面轮询 token endpoint，校验 ID Token 和 JWKS。
7. 控制面解析 `person_id`，绑定设备身份，签发设备凭据和站点 Endpoint Token。

OIDC 一中心三网关的接入建议见 `docs/superpowers/specs/2026-10-19-oidc-one-center-three-gateways-proposal.md`。核心是“一个逻辑 issuer + 三个网络接入点”，接入点不签发 Token。

### Endpoint Token

Endpoint Token 是按站点签发的短期签名令牌：

- `iss`：控制面标识。
- `aud`：`prelay-gateway`。
- `sub`：全局设备身份 ID。
- `site_id`、`endpoint_id`。
- `iat`、`nbf`、`exp`、`jti`、`kid`。

控制面签名私钥在 KMS；网关通过配置快照获得验签公钥，本地验证签名、站点、有效期、撤销列表，再确认 Endpoint 和站点策略。默认有效期 7 天，客户端通过网关管理边缘刷新。

单个站点令牌泄露不会扩散到其他站点；撤销通过短有效期和 `jti` 列表双重约束。控制面不在每次 LLM 请求时被回调。

## 配置分发

控制面为每个站点维护独立的 `config_version`。网关通过出站长轮询获取：

```text
GET /internal/v1/config?since=<site_config_version>&wait=25s
```

配置快照包含：

- 版本、站点、生成时间、租约 `valid_until`。
- 最低网关版本。
- 配置签名公钥、Endpoint Token 验签公钥和 `kid`。
- 模型目录版本和哈希。
- 本站可用的 Endpoint、模型路由、Provider 元数据。
- 本站可用的 Provider 密钥信封。
- 站点策略和撤销的 `jti` 列表。

控制面签名快照，网关先验签再在事务中应用；失败保留 last known good。租约默认 7 天，网关通过心跳或轮询续租；租约过期后拒绝新请求，已建立的流继续完成，避免站点撤销后无限期使用旧配置。

控制面多副本通过 PostgreSQL `LISTEN/NOTIFY` 感知版本变化，失败时退化为轮询。内部 API 使用 `X-Prelay-Api-Version`，控制面至少兼容当前和前一个网关版本。

## 用量与统计

### 网关侧

网关在请求开始时写本地 `activities`，终态更新和 outbox 写入在同一事务完成。每条记录分配 `site_epoch + site_seq`：

- `site_epoch` 在站点首次初始化时生成；数据库从备份恢复或重建后生成新 epoch。
- `site_seq` 在单个 epoch 内单调递增，在终态事务中分配。

outbox 批量推送：

```text
POST /internal/v1/usage/batches
```

记录只包含统计元数据：站点、活动、身份、`person_id`、时间、协议、模型、Provider、状态、Token、缓存 Token、延迟、首 Token、是否流式、上游请求 ID。不同步提示词、输出、`error_message`、`metadata_json`、Provider 密钥、设备凭据和 Endpoint Token。

控制面唯一键 `(site_id, site_epoch, site_seq)` 和 `(site_id, activity_id)`。重复批次忽略，修正只接受更高 `revision`。网关收到 ack 后标记已发送，保留 payload 30 天用于重放。

每站点单发送者按 `site_seq` 顺序发送，保证 `last_acked_seq` 是连续水位。PostgreSQL sequence 回滚空洞允许，重放以 `last_acked_seq`/`last_sent_seq` 范围为准。

控制面从备份恢复后，按 `(site_id, site_epoch)` 比较 `last_ingested_seq`，要求网关在保留窗口内重放缺失区间。30 天窗口外的缺口记录为审计边界。

### 控制面统计

`usage_facts` 保存请求级事实，按月份和站点分区；`usage_hourly` 保存小时汇总，可从事实重建。修正事实时重算受影响小时桶。

统计口径：

- `personal`：当前设备身份；OIDC 后按 canonical `person_id`。
- `team`：控制面管理的全部用户，跨所有已接入站点聚合；OIDC 前按设备身份聚合，OIDC 后按 `person_id` 合并同一人员的多设备；不引入管理员/审计员角色。

统计口径与现有 `ModelStatsScope` 一致，只有 `personal` 和 `team`，不新增 `site`/`global` scope。

维度：时间、`person_id`、设备身份、模型、Provider、协议、状态。`site_id` 只用于数据归属和幂等，不作为统计口径。事实时间存 UTC，展示时区默认 `Asia/Shanghai`。

团队统计最终一致；本地统计实时。控制面统计查询可走只读副本。

保留策略：`usage_facts` 默认 13 个月；`usage_hourly` 默认 5 年；网关活动和内容默认 30 天；outbox 30 天。

## 客户端

客户端保持管理台职责，不转发 AI 请求。跨站点配置由 profiles 和 Agent 配置同步完成。

### 站点目录

控制面维护站点目录，网关通过 `/api/sites` 下发。安装包/GPO 预置 bootstrap URL；客户端首次启动拉取站点目录并生成 profiles。网关缓存最近一次目录，控制面不可用时仍可显示已知站点。

### 凭据与令牌

- 设备凭据全局一份，保存在 Windows 凭据保护存储。
- Endpoint Token 按站点签发，每站点一份。
- 客户端为每个 profile 保存站点、URL、令牌引用和最近可用状态。
- 客户端随登录启动，令牌过期前通过当前网关管理边缘刷新。
- 默认不自动回显完整 Token；手工配置第三方工具时提供显式显示/复制，写本地审计，不写日志。

### 自动切换

客户端按“上次成功站点 -> 当前可达本地站点 -> 配置的回退站点 -> 手动选择”的顺序选择 profile。切换后把当前站点 URL 和 Token 写入 Codex、OpenCode 等 Agent 配置；运行中的 Agent 可能需重启，客户端提示或重启其托管进程。

客户端不引入本地代理；Agent 仍直接访问站点网关。OIDC 登录走设备授权流程。

## 网络与域名

| 组件 | 域名示例 | DNS | 访问来源 |
|---|---|---|---|
| 控制面 | `control.prelay.internal` | 私网 DNS | 三个网关 |
| 总部网关 | `ai-hq.corp.example` | 内网 DNS | 总部客户端 |
| 分公司网关 | `ai-branch.corp.example` | 内网 DNS | 分公司客户端 |
| 公网网关 | `ai.example.com` | 公网 DNS | 外网客户端 |

网络流向：

- 客户端 -> 本站网关 443。
- 网关 -> 控制面 443（配置、用量、心跳、Token 刷新）。
- 网关 -> 上游 Provider 443。
- 控制面 -> 中心 PostgreSQL/KMS 私网。
- 网关 -> 本地 PostgreSQL 私网。
- 控制面不主动连接网关。

客户端到网关使用站点证书；网关到控制面使用 mTLS；控制面到 PostgreSQL/KMS 使用私网 TLS。

## 高可用与故障

- 控制面 2+ 无状态副本，PostgreSQL 主备 + PITR，KMS/Vault 独立部署。
- 每个站点网关 2+ 副本，本地 PostgreSQL 主备；配置同步和 outbox 发送分别用 advisory lock 选主。
- 公网网关与控制面分开部署、分开扩容；控制面发布不打断公网 LLM 流。
- 长轮询负载均衡空闲超时大于 25 秒，网关有重连退避。

故障语义：

| 故障 | 影响 | 行为 |
|---|---|---|
| 控制面不可用 | 管理、OIDC、新注册、团队统计不可用 | 网关用缓存继续已授权请求；outbox 积压 |
| 中心数据库不可用 | 控制面不可用 | 网关不受影响 |
| KMS 不可用 | 新信封/配置变更失败 | 已解封内存密钥继续；网关重启后无法解封新密钥 |
| 站点网关不可用 | 该站点断 | 其他站点不受影响 |
| 站点数据库不可用 | 该站点拒绝新请求 | 已建立流完成；恢复后清扫超时活动 |
| VPN/专线不可用 | 站点无法同步 | 本地缓存继续；租约到期后停止新请求 |
| 站点密钥泄露 | 该站点 Provider 密钥可能暴露 | 撤销站点、重新信封、轮换 Provider 密钥 |
| Endpoint Token 泄露 | 只能在签发站点使用 | 撤销 `jti`，短有效期兜底 |

网关数据库恢复后必须生成新 `site_epoch`；站点重建沿用原 `site_id`，不能复用旧 ID。

## 可观测性与灾难恢复

- 控制面暴露每站点配置版本、心跳、用量接收延迟和缺口、配置快照耗时、KMS/数据库错误、统计查询延迟。
- 网关暴露请求/成功率/Token/延迟、上游错误、outbox 积压、配置版本和租约剩余、Token 验签失败、本地数据库健康。
- 日志结构化，包含站点、请求 ID、`activity_id` 和错误码，不包含 Token、Provider Key、设备凭据、提示词或完整响应。
- 告警覆盖站点心跳丢失、outbox 积压、`site_epoch` 缺口、配置租约不足、KMS/数据库错误和异常 5xx。
- 控制面 PostgreSQL 使用 PITR；网关 PostgreSQL 使用 PITR 或全量加 WAL 备份。
- 控制面恢复后按 `(site_id, site_epoch)` 对账并请求 30 天窗口内重放；网关恢复生成新 epoch；站点重建沿用原 `site_id`。
- RPO/RTO 由运维按业务中断容忍度确定，设计提供机制，不虚构具体分钟数。

## 安全

- 控制面不对最终用户开放，只接受站点 mTLS；用户管理请求经本站网关转发。
- 控制面不保存提示词、模型输出、Provider 明文密钥和设备凭据明文。
- Provider 密钥使用 KMS 主密钥 + 站点公钥两层保护；站点私钥在本地 KMS/TPM。
- Endpoint Token 按站点签名，网关本地验签。
- 配置快照签名；未知 `kid` 拒绝，不做无签名降级。
- 管理操作写追加式审计日志；日志禁止 Token、Provider Key、设备凭据、提示词和完整响应。
- 限流按用户、站点、模型、Provider 分层；不实现配额、计费或结算。
- 提示词和输出默认留在站点；如合规要求集中审计，使用独立端到端加密审计通道。

## 数据模型

控制面主要表：

- `sites`、`site_config_versions`
- `identities`、`identity_subject_bindings`
- `oidc_providers`、`oidc_login_sessions`、`oidc_jwks_cache`
- `providers`、`provider_shares`、`provider_site_policies`
- `provider_secret_master`、`provider_secret_envelopes`
- `endpoints`、`endpoint_models`、`endpoint_token_issuances`
- `usage_facts`、`usage_hourly`
- `audit_log`、`signing_keys`

网关本地主要表：

- `activities`、`activity_contents`
- `outbox`、`config_state`
- `provider_secret_cache`、`revoked_token_cache`
- `sync_state`（`site_epoch`、配置版本、已发送/已确认序号）

关键约束：

- 全局 Provider 只有一份实体；站点只保存本站信封，不复制 Provider。
- `providers.owner_identity_id` 非空；只有所有者能编辑/轮换/删除，其他用户按分享关系只读使用。
- `endpoints.owner_identity_id` 非空；Endpoint 全局保存一份，Endpoint Token 按站点签发。
- Provider 和 Endpoint 的名称在所有者范围内唯一；不同用户可同名。
- `usage_facts` 唯一键 `(site_id, site_epoch, site_seq)` 和 `(site_id, activity_id)`。
- `provider_secret_envelopes` 唯一键 `(provider_id, site_id)`。
- `endpoint_token_issuances.jti` 唯一。
- `site_id` 不可复用；数据库恢复必须生成新 `site_epoch`。
- 网关 `activities.identity_id` 是全局身份 ID，不使用指向控制面身份表的外键。

## 迁移

从当前单实例迁移到控制面/网关是一次性迁移，不保留双写：

1. 控制面导入身份、Provider、Endpoint、模型映射和分享关系。
2. 使用旧 `ENCRYPTION_KEY` 解密现有 Provider Key，写入 KMS 主副本，生成站点信封。
3. 为每个站点导入活动和内容，生成初始 `site_epoch`，历史活动回填 `usage_facts`。
4. 客户端把 `relay_url` 转成 profile，保留设备凭据，为每个站点签发新的 Endpoint Token。
5. 迁移完成后销毁旧 `ENCRYPTION_KEY`，撤销旧 Token。

迁移需要停机或只读窗口；迁移后对账身份、Provider、Endpoint、Token、活动和统计。

## 项目规则变更

实施前必须同步修改：

- 根 `AGENTS.md`：Provider Key 从单一 `ENCRYPTION_KEY` 改为 KMS 主副本 + 站点信封；服务端拆为控制面/网关。
- `prelay-server/AGENTS.md`：控制面/网关职责、四库、配置签名、Token 验签和 Provider 信封。
- `prelay-client/AGENTS.md`：允许管理多个站点 profile 和站点目录；继续保持不直接转发 AI 请求。
- `prelay-server/docs/architecture/database.md`：补充控制面/网关独立 schema 和同步边界。
- 部署/发布文档：管理区、VPN、mTLS、KMS/TPM、四库、域名、证书和兼容矩阵。

`prelay-protocol` 继续是客户端可见 DTO 和稳定错误码的唯一来源。控制面/网关内部 API 使用独立 crate 或模块，不复制到 `prelay-protocol`。

## 非目标

- 不共享 PostgreSQL、逻辑复制、双向同步或分布式事务。
- 不把控制面放进 `/v1` 请求链路。
- 不默认集中保存提示词和模型输出。
- 不保存 Provider 明文密钥。
- 不实现配额、计费、账单或成本结算。
- 不实现控制面多主写入。
- 不引入 Kafka、服务网格或额外消息队列。
- 不在客户端实现本地 AI 转发代理。
- 不为每个站点复制 Provider/Endpoint 配置。

## 验证

- 配置快照签名、版本、租约和兼容矩阵。
- Endpoint Token 站点/身份/Endpoint/有效期/撤销校验。
- Provider 主副本、站点信封、站点密钥轮换和撤销。
- 全局 Provider 只配置一次，三个站点通过快照使用同一份元数据。
- 配置长轮询、版本变更通知、last known good。
- outbox 幂等、`site_epoch + site_seq` 缺口检测和重放。
- 控制面恢复后的用量对账。
- 个人/团队统计，控制面不可用时的本地降级。
- OIDC 一中心三网关的 provider 选择、issuer 映射、`person_id` 解析和 JWKS 轮换。
- 迁移前后的身份、Provider、Endpoint、Token、活动和统计对账。

本文件是设计，不代表上述验证已经执行。

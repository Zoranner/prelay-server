# OIDC 一中心三网关接入建议

## 结论

建议采用“一个身份中心 + 三个网络接入点”，而不是“一个中心 + 三个 OIDC issuer”。

- 中心是唯一的用户目录、认证、MFA、会话和令牌签发方。
- 三个网关只负责网络可达性、TLS 终止或透传、访问控制、限流和审计元数据。
- 网关不保存用户、不保存令牌、不持有 OIDC 签名私钥、不签发自己的 OIDC Token。
- 对外只暴露一个逻辑 issuer，例如 `https://sso.example.com`。三个网络通过分离 DNS 把这个域名解析到各自网关。
- 如果必须使用三个站点域名，域名只作为访问入口，令牌里的 `iss` 仍然保持中心逻辑 issuer；不要让三个网关成为三个 issuer。

最优雅的实现不是写一个自定义 OIDC 网关，而是用标准 L4/TLS 透传代理：客户端连接本地网关，网关把 TCP 连接转发到中心，中心用自己的证书终止 TLS。网关看不到明文 Token，也不需要理解 OIDC 协议。

## 目标与约束

- 总部、分公司、公网三个网络互相隔离，内网客户端不能直接访问中心 IdP。
- 三个网络各有自己的域名和证书策略。
- 同一个用户在不同网络登录后必须得到同一个 `person_id`。
- 网关故障不能导致身份数据分叉；中心故障不能通过本地签发 Token 来“兜底”。
- 身份系统要同时服务 Prelay 和其他内部应用，不能只为 Prelay 定制。
- 边缘网关不能成为第二个用户目录、第二个签名中心或第二个会话中心。

## 推荐拓扑

```text
                         ┌──────────────────────────────┐
                         │ OIDC Center                  │
                         │ 用户目录 / MFA / 会话 / 签名  │
                         │ JWKS / Discovery / 审计      │
                         └──────────────▲───────────────┘
                                        │ 私网 / VPN / mTLS
              ┌─────────────────────────┼─────────────────────────┐
              │                         │                         │
   ┌──────────┴─────────┐   ┌──────────┴─────────┐   ┌──────────┴─────────┐
   │ 总部 OIDC 接入点    │   │ 分公司 OIDC 接入点  │   │ 公网 OIDC 接入点    │
   │ L4 / TLS 透传       │   │ L4 / TLS 透传       │   │ L4 / TLS 透传       │
   │ 无用户、无签名、无Token│ │ 无用户、无签名、无Token│ │ 无用户、无签名、无Token│
   └──────────▲─────────┘   └──────────▲─────────┘   └──────────▲─────────┘
              │                         │                         │
         总部客户端                 分公司客户端                 外网客户端
```

推荐的三条规则：

1. 客户端只认一个 issuer 域名。
2. 分离 DNS 把 issuer 域名解析到当前网络最近的接入点。
3. 接入点只转发连接，不解释、不改写、不签发 OIDC Token。

## 为什么不是三个 issuer

三个独立 issuer 会带来：

- 同一用户在三个网络可能得到三个不同的 `(iss, sub)`，跨站统计和跨应用关联无法直接合并。
- 每个网关都要有自己的签名密钥、JWKS、轮换、撤销和客户端注册。
- 每个网关都要处理 pairwise `sub`、会话、登出和跨 issuer 信任。
- 三个网关从“网络接入点”变成“三个轻量 IdP”，攻击面和运维成本显著增加。
- Prelay 和其他应用需要维护 issuer 映射、人员 homelink 和跨 issuer 的权限模型。

只有在三种情况下才值得做独立 issuer：

- 三个网络有完全独立的用户目录，短期无法合并。
- 网关必须离线签发 Token，中心不可达时仍要登录。
- 法规要求每个网络独立保存身份数据和签名密钥。

如果只是解决“内网客户端到不了中心”，不需要独立 issuer；用网络接入点就够了。

## 为什么不是 OIDC Broker

OIDC Broker 网关会终止上游会话、重新签发本地 Token。它适合多 IdP 联邦或复杂租户隔离，但在这里会引入：

- 中心 Token 和网关 Token 两套生命周期。
- 网关签名密钥、JWKS、客户端注册和撤销链路。
- 中心与网关之间的身份映射和会话同步。
- 网关被攻破后可伪造本地 Token 的风险。

如果中心本身已经能提供统一用户目录和认证，Broker 是多余的中间层。

## 域名与 issuer 策略

### 首选：一个 issuer 域名加分离 DNS

中心对外声明：

```text
issuer = https://sso.example.com
```

三个网络的 DNS 分别把 `sso.example.com` 解析到本地接入点：

| 网络 | `sso.example.com` 解析到 | 实际转发 |
|---|---|---|
| 总部 | 总部接入点 | 中心 IdP |
| 分公司 | 分公司接入点 | 中心 IdP |
| 公网 | 公网接入点 | 中心 IdP |

优点：

- 中心只签发一种 `iss`，所有客户端看到同一个 issuer。
- Cookie、会话、登出、JWKS 和客户端注册都围绕同一个域名。
- 接入点可以是标准 L4 代理，不需要 OIDC 逻辑。
- 同一用户在不同网络得到相同 `sub`。

### 次选：站点别名只作为访问入口

如果分离 DNS 不可行，可以使用：

```text
https://sso-hq.corp.example
https://sso-branch.corp.example
https://sso.example.com
```

但必须满足：

- 三个别名都指向中心或本地接入点，Token 的 `iss` 仍然是 `https://sso.example.com`。
- 客户端验证 Token 时使用 canonical issuer，不使用别名。
- Discovery 文档必须从 canonical issuer 获取；不要让接入点通过站点别名返回一个 `issuer` 与检索 URL 不一致的 discovery，严格 OIDC 客户端会拒绝。
- Cookie 和会话仍以 canonical issuer 为准；如果做不到，站点别名只用于设备验证页面或浏览器入口，不作为 OIDC issuer 或 discovery 地址。

如果别名导致 `iss`、Cookie 或 Discovery 不一致，就不要使用别名，优先解决 DNS。

### 不推荐：三个 issuer 域名

三个 issuer 域名会让 Prelay 和其他应用必须做跨 issuer 人员映射。除非确实需要独立用户目录或离线签发，否则不要采用。

## 接入点职责

### 推荐：L4/TLS 透传

接入点只做：

- 接收本地网络的 TCP 连接。
- 按目标地址和端口转发到中心 IdP。
- 做网络 ACL、连接数限制、健康检查和基础 DDoS 防护。
- 保留客户端源 IP（PROXY protocol 或等价机制，按网络设备支持情况）。
- 记录连接元数据，不记录 Token、Cookie 或请求正文。

这种方式下：

- 中心用自己的证书终止 TLS，接入点看不到明文 Token。
- 不需要在三个网络分发 OIDC 证书。
- 如果中心要求 OIDC client 使用 mTLS，L4 透传能保留端到端客户端证书；L7 终止 TLS 后还要额外转发或重签客户端证书，通常不划算。
- 接入点可以标准化为 Envoy/Nginx stream 或硬件负载均衡配置，不需要 OIDC 团队写业务代码。

### 可选：L7 反向代理

如果安全策略要求 WAF、路径级限流或 HTTP 审计，可以使用 L7 反向代理，但必须遵守：

- 保留原始 `Host` 为 canonical issuer，或正确设置 `X-Forwarded-Host` 和 `X-Forwarded-Proto`，并只信任接入点自己的头。
- 不缓存用户会话、Token、授权码或设备码。
- 不记录 Authorization、Cookie、Token 响应和设备码请求正文。
- 不修改 Token、`iss`、`sub`、`aud`、`nonce` 或签名。
- 不持有 OIDC 签名私钥，不签发本地 Token。
- 设备码流程只允许把 `verification_uri` 重写到本地入口；不能改变 `iss`。
- 接入点到中心使用 mTLS 或私网链路。

## 设备授权流程（Device Authorization Grant）

内网客户端无法访问中心 IdP 的浏览器页面，设备授权流程是最合适的：

1. 客户端向本地接入点请求 device authorization。
2. 接入点透传到中心。
3. 中心返回 `device_code`、`user_code`、`verification_uri`、`expires_in` 和 `interval`。
4. 如果使用站点别名，只有 L7 接入点可以把 `verification_uri` 重写为本地可访问地址；L4 + canonical issuer + 分离 DNS 不需要重写，`iss` 始终不变。
5. 用户在本站接入点或手机/外网设备完成登录。
6. 客户端轮询本站 token endpoint，接入点透传到中心。
7. 中心签发 Token，`iss` 始终是 canonical issuer。

优点：

- 不需要为三个站点分别注册 redirect URI。
- 客户端不需要直接访问中心。
- 用户可以在一台有浏览器的设备上完成登录。
- 接入点不需要理解授权码或 Token 内容。

如果其他应用使用 authorization code + PKCE，则为每个应用的 redirect URI 注册；接入点仍然只做透传。

## 人员键与 claims

中心必须提供稳定、非 pairwise 的 `sub`。建议：

```text
iss = https://sso.example.com
sub = 稳定、不可复用、不可猜测的用户标识
employee_id = 可选，稳定工号
name / email / groups = 展示和授权辅助
```

规则：

- `sub` 是人员主键；不要用邮箱、UPN、显示名或工号替代 `sub`。
- 不要把 `sub` 做成 per-client pairwise；如果必须 pairwise，至少提供稳定的 `employee_id` claim，并明确它的唯一性和不可变性。
- `aud` 必须包含 Prelay 的 client_id；多 audience 时校验 `azp`。
- `nonce`、`state`、`exp`、`nbf`、`iat` 按 OIDC 规范和允许的时钟偏差校验。
- `groups` 可以用于 Prelay 管理员角色，但不要用组名做人员主键。

## 客户端注册

- Prelay 控制面注册为一个 OIDC client，使用 device authorization grant。
- client secret 只保存在 Prelay 控制面的 KMS/Vault，不下发到网关。
- 三个接入点共享同一个 client_id；接入点只透传客户端认证，不替换 client_id。
- 如果安全策略要求按站点区分 client，可以注册三个 client，但必须保证三个 client 的 `sub` 或 `employee_id` 仍然指向同一个人。
- 设备码流程不需要 redirect URI；如果使用 authorization code，按应用注册精确的 redirect URI。

## 密钥与信任

- OIDC 签名私钥只在中心 HSM/KMS，不进入接入点。
- 中心发布 JWKS，`kid` 轮换有重叠窗口。
- L7 接入点可以缓存 JWKS 和 discovery，但缓存的是公钥和元数据，不是用户 Token；L4 接入点不解析 HTTP，JWKS/discovery 缓存由中心、OIDC 客户端和 Prelay 控制面负责。
- 中心和接入点之间使用私网/VPN；需要时加 mTLS。
- 接入点证书只用于本地 TLS 入口或管理接口，不用于签发 OIDC Token。

## 可用性与故障

- 中心：多副本、数据库主备、HSM/KMS、JWKS 高可用。
- 接入点：每个网络至少两个副本，放在本地负载均衡后；无状态，不共享会话。
- 中心故障：接入点返回 `503`，不本地签发 Token；已签发 Token 在有效期内继续可用。
- 本地接入点故障：该网络新登录失败；另一副本接管；不要把公网接入点当作内网客户端的透明回退，除非网络可达。
- JWKS 缓存过期：使用缓存直到 TTL；遇到未知 `kid` 先刷新，刷新失败则拒绝 Token，不做无签名降级。
- 时钟偏差：中心和接入点使用 NTP；超出允许偏差拒绝 Token 并记录原因。

## 安全边界

- 接入点不保存用户、密码、Token、授权码、设备码或会话。
- 接入点日志只记录时间、来源、路径、状态、延迟和请求 ID；不记录 Authorization、Cookie、Token 响应正文。
- L7 接入点必须防止请求头伪造，只信任自己设置的 `X-Forwarded-*`。
- 设备码、client_secret 和 Token 只通过 POST/TLS 传输；authorization code 按 OAuth 规范通过 redirect query 或 form_post 返回，接入点不得记录或缓存包含 code 的 URL。
- 登录页、同意页和 MFA 只在中心实现；L7 接入点只做透明反向代理，不修改页面内容，不实现登录逻辑。
- 中心负责暴力破解防护、MFA、会话固定防护、CSRF、PKCE 和 Token 撤销。
- L4 接入点负责网络 ACL、连接限流和基础 DDoS 防护；需要 WAF 或路径级限流时使用 L7 接入点。
- 管理接口与用户入口分离，管理接口只允许管理网/VPN。

## 与 Prelay 的接口

Prelay 控制面按站点维护 OIDC provider：

```text
site_id
issuer = https://sso.example.com
discovery_url / device_authorization_endpoint / token_endpoint / jwks_uri
client_id
client_secret_ref
canonical_issuer_id
fallback_provider_id
```

流程：

1. 客户端向本站 Prelay 网关发起 `/api/auth/device/start`。
2. Prelay 网关带 `site_id` 转发到控制面。
3. 控制面选择本站 OIDC provider，启动 device authorization。
4. 用户在本站 OIDC 接入点完成登录。
5. 控制面校验 ID Token，得到 `iss` 和 `sub`。
6. 控制面计算 `person_id = canonical_issuer + sub`，绑定设备身份。
7. 控制面签发 Prelay 设备凭据和站点 Endpoint Token。

如果 OIDC 团队选择三个 issuer，Prelay 仍然可以通过 `canonical_issuer_id` 和稳定 `employee_id` 映射到 `person_id`，但这会增加映射和运维成本；不作为首选。

## 反模式

- 三个接入点各自签发 Token，形成三个 issuer。
- 接入点保存用户目录或密码。
- 接入点保存 OIDC 签名私钥。
- 接入点缓存用户 Token 或会话。
- 接入点改写 `iss`、`sub`、`aud` 或签名。
- 用邮箱、UPN、显示名做人员主键。
- 使用 pairwise `sub` 但不提供稳定人员 claim。
- 中心不可达时由接入点本地签发 Token 兜底。
- 为设备码流程注册三个 redirect URI，增加不必要的配置。
- 让内网客户端直接访问中心 IdP，破坏网络隔离。

## 需要 OIDC 团队确认的决策

1. 是否接受“一个逻辑 issuer + 三个网络接入点”，而不是三个 issuer。
2. 三个网络是否可以做分离 DNS，把 canonical issuer 解析到本地接入点。
3. 接入点选择 L4/TLS 透传还是 L7 反向代理；默认建议 L4。
4. `sub` 是否稳定、非 pairwise；如果 pairwise，稳定的员工号 claim 是什么。
5. 是否支持 device authorization grant，以及 `verification_uri` 是否允许按接入点域名生成或重写。
6. client 注册是一个 Prelay client 还是按站点三个 client；secret 如何存放在 KMS。
7. JWKS 轮换、时钟偏差、Token 有效期、刷新和撤销策略。
8. 中心故障时接入点返回 `503`、不本地签发 Token 的语义是否接受。
9. 接入点日志、审计和限流边界。
10. 如果必须使用三个 issuer，跨 issuer 的人员映射和 homelink 由谁负责。

## 结论

最优雅的 OIDC 架构是“中心签发、接入点转发”：

- 一个逻辑 issuer，三个网络接入点。
- 中心是唯一用户目录、唯一签名方和唯一会话权威。
- 接入点是标准 L4/TLS 透传或薄 L7 反向代理，不保存用户、不保存 Token、不签发 Token。
- 设备授权流程解决内网客户端无法访问中心的问题，不需要三个 redirect URI。
- 稳定 `sub` 或稳定员工号 claim 是跨站人员统计的前提。

这套设计把复杂度放在中心，把边缘保持为可替换的网络组件；既适合 Prelay，也适合后续其他内部应用接入。

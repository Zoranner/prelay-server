# 供应商额度设计

## 目标

供应商列表用一列紧凑展示三类额度：

- 套餐额度：供应商返回的 5 小时、周、月窗口。
- 余额额度：供应商账户剩余金额。
- 共享额度：拥有者直接共享自己的额度，或给被共享者单独设置日、周、月限额。

不新增独立额度页面。共享设置只负责选择身份和额度规则，额度查看仍由供应商列表的额度列承担。

## 协议模型

`ProviderQuota` 是一张卡，不再是互斥枚举：

- `plan`：套餐名，可选。
- `windows`：零到多个额度窗口。
- `balance`：零到一个余额读数。
- `error`：查询失败时的稳定错误码和消息。
- `read_at`：读数时间。
- `stale`：是否展示上次成功读数。

`QuotaWindow` 统一用 `used_percent` 表示已用百分比，同时保留厂商原样的 `amount`、`limit`、`unit`、`display`、`resets_at` 和 `pool`。`aside` 表示不影响模型调用的旁路窗口，例如 GLM 的 MCP 月额度。

`ProviderQuotaResponse.mode` 区分三种归属：

- `own`：自己的供应商，展示拥有者套餐或余额。
- `shared_unlimited`：直接共享，展示拥有者套餐或余额。
- `shared_limited`：限额共享，展示当前身份自己的一份额度。

## 目录声明

额度取数方式由供应商目录声明：

```toml
[providers.quota]
adapter = "glm_coding_plan"
url = "https://open.bigmodel.cn/api/monitor/usage/quota/limit"
auth = "raw"
timeout_secs = 8
```

- `adapter` 只接受已注册的命名适配器，未知值在目录加载时失败。
- `url` 支持绝对地址或以 `/` 开头的相对地址；相对地址按供应商 `base_url` 的 origin 解析。
- `auth` 支持 `bearer` 和 `raw`。
- 没有 `[providers.quota]` 表示该供应商未接入额度查询。

目录只声明取数方式，不保存密钥、账号上下文或手填额度。

## 供应商额度计算

| 供应商 | 端点 | 读数字段 | 展示 |
| --- | --- | --- | --- |
| DeepSeek | `GET /user/balance` | `is_available`、`balance_infos[]` | 多币种余额 |
| GLM Coding Plan | `GET /api/monitor/usage/quota/limit` | `data.limits[]` 的 `type`、`unit`、`number`、`percentage`、`nextResetTime` | 5 小时、周、MCP 月；百分比为已用 |
| Kimi Code | `GET /coding/v1/usages` | `limits[]`、`usage`、`remaining`、`resetTime` | 动态短窗口和 7 天 |
| MiniMax Token Plan | `GET /v1/token_plan/remains` | `model_remains[]`、`base_resp` | 5 小时和 7 天；剩余百分比翻成已用 |
| Command Code | `GET /alpha/billing/credits` | `windowLimits.fiveHour/weekly`、`credits` | 5 小时、7 天、月度 credits 和 USD 余额 |
| OpenCode Go | `GET /zen/go/v1/usage` | `usage.rolling/weekly/monthly.percent` | 5 小时、7 天、月；百分比为已用 |
| GoToken | 无公开额度接口 | 无 | 显示未接入 |

千问 Token Plan 已退役，不再出现在目录和新建供应商列表。

## 共享额度

共享记录按 `provider_id + grantee_identity_id` 保存一份可选的 `quota_json`：

- `NULL` 表示直接共享，被共享者看到拥有者的套餐或余额。
- 非空数组表示限额共享，被共享者只看到自己的一份额度。
- 窗口只允许 `day`、`week`、`month`，同一被共享者每种窗口最多一条。
- 每个窗口独立设置 token 上限、是否累计、清零时间和周期边界。
- 多个窗口同时启用时，实际可用额度取所有窗口剩余的最小值。
- 累计只把上一同类周期的剩余额度带入本期，最多带入一个基础周期。
- 日、周、月边界按 `+08:00` 计算。
- 共享关系建立前的活动不计入这次共享的本期已用。

请求进入时按每个启用窗口预留估算 token。额度不足返回 429，并短暂冷却该供应商；预留跟随后端响应体生命周期，流式响应结束或断开后才释放。当前预留是进程内状态，不跨服务实例共享。

共享额度按活动记录的总 token 统计；没有 token 计数的活动不计入共享限额。创建者查看被共享人用量暂不实现。

## 展示

供应商列表增加紧凑的额度列：

- 套餐：按实际窗口数画同心圆环，数值为最紧窗口的剩余百分比。
- 余额：显示币种和金额；可用余额为零或不可用时显示警告色。
- 共享限额：显示最紧窗口的剩余百分比和进度条。
- 直接共享：沿用拥有者的套餐圆环或余额。
- 状态：未接入、待配置、查询失败、旧读数。

浮层展示全部窗口的重置时间和剩余量；有 `display` 时同时展示厂商计数或共享的已用/上限。共享抽屉只配置共享，不显示用量统计。

## 取数与缓存

- 同一身份的可见供应商额度并发拉取，单家默认四秒超时。
- 成功读数缓存 60 秒，错误读数缓存 15 秒。
- 上游不可达或响应无效时，5 分钟内保留上次成功读数并标记 `stale`。
- 缓存键包含供应商 id、供应商地址、额度地址和密钥摘要。

## 验证边界

- 六个解析器用契约 fixture 验证，未使用真实上游密钥。
- GLM 只支持个人版 Coding Plan；团队版不在范围。
- GoToken 保持未接入。
- 共享额度的并发保护只覆盖单个服务进程。

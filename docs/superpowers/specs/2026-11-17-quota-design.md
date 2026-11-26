# 供应商额度设计

> 2026-11-22 起，额度协议与展示以 `2026-11-22-quota-contract-design.md` 为准。

## 目标

供应商列表用一列紧凑展示三类额度：

- 套餐额度：供应商返回的 5 小时、周、月窗口。
- 余额额度：供应商账户剩余金额。
- 共享额度：拥有者直接共享自己的额度，或为这次共享设置一套日、周、月限额。

不新增独立额度页面。共享设置只负责选择用户和额度规则，额度查看仍由供应商列表的额度列承担。

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
- `shared_limited`：限额共享，展示同一套共享规则下当前用户自己的用量。

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
| Command Code | `GET /alpha/billing/credits`、`GET /alpha/billing/subscriptions` | `windowLimits.fiveHour/weekly`、`credits.monthlyCredits`、`data.planId/currentPeriodEnd` | 5 小时、7 天、月；月度 cap 按套餐或 5 小时 cap 推导，重置取计费周期结束 |
| OpenCode Go | `GET /zen/go/v1/usage` | `usage.rolling/weekly/monthly.percent` | 5 小时、7 天、月；百分比为已用 |

千问 Token Plan 和 GoToken 已退役，不再出现在部署目录和新建供应商列表。

## 共享额度

共享额度是供应商级的一条规则，保存在 `identity_provider_configs.share_quota_json`，不按被共享用户分别保存：

```json
{ "amount": 100000000, "accumulate": "week" }
```

- `NULL` 表示直接共享，被共享者看到拥有者的套餐或余额。
- 非 `NULL` 表示限额共享，所有被共享用户使用同一条规则。
- 每个被共享用户按自己的活动独立计算已用，不把多个用户的活动合并成一个额度池。
- `amount` 是每天注入的 token 额度；`accumulate` 是累积范围，只允许 `none`、`week`、`month`。
- 本周期已累积上限 = `amount` × 本周期已过天数；周总额 = `amount` × 7；月总额 = `amount` × 当月实际天数（28/29/30/31）。
- 实际可用额度取当天剩余与周期总额剩余的最小值。日额度每天 00:00 注入一份，周期结束整体归零。
- 周期边界按 `+08:00` 计算：日 00:00、周一 00:00、每月 1 日 00:00。
- 指定用户的起算点是授权建立时间，所有用户的起算点是供应商创建时间；起算点之前的活动不计入本期已用，累积也从起算当天开始。
- 例：每天 1 亿、周内累积。周一周二未用时，周三上限 3 亿；周三用尽这 3 亿后，周四上限 4 亿、可用 1 亿；下周一 00:00 回到 1 亿。
- 旧版本把规则存成窗口数组，读取时折叠成上面的单条规则：`amount` 取日窗口的 `limit`，`accumulate` 取日窗口自己的标记——`false` 折叠成 `none`，`true` 且有月窗口折叠成 `month`，否则折叠成 `week`。周、月窗口的 `limit` 不参与计算。下一次保存即写成新格式。
- 旧规则没有日窗口时，按周上限 ÷ 7、月上限 ÷ 30 向下取整折算成 `amount`。
- 折叠时如果周上限低于 7 × 日额度、或月上限低于 30 × 日额度，说明不是界面产物，返回 `invalid_provider_sharing`，不按日额度放大。
- 迁移后 `accumulate = none` 的共享没有周期窗口：界面第二圈是灰环，悬浮里没有本周或本月行。
- 旧周、月窗口的 `accumulate=true`（上一周期结转）不再支持。
- 共享建立当天按一整天计，当天建立也能用满一份日额度。
- `quota` 的形状与旧客户端不兼容：服务端与客户端必须同版发布，不保留旧形状的读取兼容。

共享接口中的 `users` 只表示被共享用户，额度规则放在共享设置顶层的 `quota`：

- `GET /api/providers/:provider_id/sharing` 返回 `visibility`、`users` 和 `quota`，仅创建人可读。
- `PATCH /api/providers/:provider_id/sharing` 接收 `visibility`、`users` 和 `quota` 全量替换。

请求进入时按窗口预留估算 token。额度不足返回 429，错误体为 OpenAI 格式，`code` 固定为 `shared_quota_exhausted`，`message` 只带可用的 token 数和重置时间。额度不足会短暂冷却该供应商；预留跟随后端响应体生命周期，流式响应结束或断开后才转为待物化额度。后台物化进度达到该请求的 WAL 提交序列后释放待物化额度；没有序列跟踪时按 15 秒 TTL 兜底。当前预留是进程内状态，不跨服务实例共享。

共享额度按活动记录的总 token 统计；没有 token 计数的活动不计入共享限额。创建者查看被共享人用量暂不实现。

## 展示

供应商列表增加紧凑的额度列：

- 套餐：按实际窗口数画同心圆环，数值为最紧窗口的剩余百分比。
- 余额：显示币种和金额；可用余额为零或不可用时显示警告色。
- 共享限额：显示最紧窗口的剩余百分比和进度条。限额共享固定返回当天窗口，累积时段再返回本周或本月总额窗口。
- 直接共享：沿用拥有者的套餐圆环或余额。
- 状态：未接入、待配置、查询失败、旧读数。

浮层展示全部窗口的重置时间和剩余量；有 `display` 时同时展示厂商计数或共享的已用/上限。当天窗口的上限是已累积额度，本周/本月窗口的上限是周期总额，两者的“已用”都是本周期用量。共享抽屉只配置每天额度和累积范围，不显示用量统计。

## 取数与缓存

- 同一身份的可见供应商额度并发拉取，单家默认四秒超时。
- 成功读数缓存 60 秒，错误读数缓存 15 秒。
- 上游不可达或响应无效时，5 分钟内保留上次成功读数并标记 `stale`。
- 缓存键包含供应商 id、供应商地址、额度地址和密钥摘要。

## 验证边界

- 六个解析器用契约 fixture 验证，未使用真实上游密钥。
- GLM 只支持个人版 Coding Plan；团队版不在范围。
- 共享额度的并发保护只覆盖单个服务进程。

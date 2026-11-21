# 额度契约重构设计

## 目标

额度协议只承载语义，服务端只计算事实，客户端只决定文案。套餐窗口、余额、共享限额、预留和界面展示共用同一套模型，不再用展示字符串表达窗口含义。

## 范围

- `prelay-protocol` 的额度 DTO。
- `prelay-server` 的额度适配器、共享额度计算、预留。
- `prelay-client` 的额度列、共享额度规则和窗口展示。

不包含：供应商共享用户管理、独立额度页面、MCP 接入、共享额度 429 错误的 API 错误码改造。

## 已确认决策

- 不做向后兼容，三个仓库同批发布。
- 删除 `QuotaWindow.label`、`QuotaWindow.aside`、`QuotaWindow.pool`、`QuotaBalance.text`。
- MCP 月额度不进入协议；GLM 的 `TIME_LIMIT` 窗口不再产出。
- 个人套餐窗口只传百分比；`remaining/limit/unit` 只用于共享限额的 token 余量。
- Command Code 的 5 小时/周/月按百分比展示，不传美元窗口金额；额外 credits 仍作为余额。
- Kimi 解析 `usages` 的 5 小时、周、月窗口，并把 `booster_wallet` 作为余额。
- MiniMax 周窗口按 `current_weekly_status == 1` 判断是否存在。

## 协议模型

```rust
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaWindowKind {
    Custom,
    Rolling,
    Day,
    Week,
    Month,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaUnit {
    Tokens,
    Usd,
    Credits,
    Requests,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QuotaWindow {
    pub kind: QuotaWindowKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_secs: Option<i64>,
    pub used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<QuotaUnit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QuotaBalance {
    pub parts: Vec<QuotaBalancePart>,
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct QuotaBalancePart {
    pub currency: String,
    pub amount: String,
}
```

`QuotaError.message` 只做诊断，客户端按 `code` 出文案。

## 窗口类型

| kind | 用途 | span_secs |
| --- | --- | --- |
| `rolling` | 5 小时或厂商动态短窗口 | 必填 |
| `day` | 共享日窗口 | 86400 |
| `week` | 套餐周窗口、共享周窗口 | 604800 |
| `month` | 套餐月窗口、共享月窗口 | 空 |
| `custom` | 无法归类的窗口 | 可空 |

## 额度形态

| 形态 | 字段 | 说明 |
| --- | --- | --- |
| 套餐窗口 | `windows[]` + `kind` | 只展示百分比；个人套餐不使用 `day` |
| 共享限额 | `windows[]` + `day/week/month` + `remaining/limit/unit` | token 余量，参与预留 |
| 余额 | `balance.parts` | 按量付费/预付余额（现金、赠金、额外 credits） |
| 状态 | `error`、`stale`、`read_at` | 未接入、待配置、查询失败、旧读数 |

订阅额度和按量付费余额是两个独立体系：

- 订阅窗口只表达套餐配额和重置周期，不把账户余额折算成窗口。
- 余额只表达按量付费/预付余额，不参与最紧窗口计算，也不是套餐配额。
- 两者不合并、不互相推导；Command Code 的美元窗口仍然是订阅配额，和 purchased/free credits 分开。

## 供应商映射

| 供应商 | 窗口 | 余额 | 说明 |
| --- | --- | --- | --- |
| DeepSeek | 无 | 多币种余额 | 不变 |
| GLM | `rolling`（`unit=3`，span 为 N 小时）、`week`（`unit=6`） | 无 | 官方套餐只有 5 小时和每周；跳过 `TIME_LIMIT`；其余 unit 记为 `custom` |
| Kimi | `limits[]` 动态窗口、`usage` 周窗口、`usages.limit_5h/limit_7d/limit_month_total/limit_month_code` | `booster_wallet` | 月窗口优先 `limit_month_total`，没有时用 `limit_month_code` |
| MiniMax | `rolling`（5 小时）、`week`（仅 `current_weekly_status == 1`） | 无 | 只取 `general` 桶；视频/图像桶不属于模型调用额度；`status == 2` 时已用置 100 |
| Command Code | `rolling`（5 小时）、`week`、`month` | purchased + free credits | 窗口只有百分比，`remaining/limit` 为空 |
| OpenCode Go | `rolling`、`week`、`month` | 无 | 只有百分比 |
| 共享限额 | `day`、`week`、`month` | 无 | 只有这类窗口带 token 的 `remaining/limit/unit` |

## 余额来源

| 供应商 | 可编程读取 | 处理 |
| --- | --- | --- |
| DeepSeek | `GET /user/balance` | 读 `is_available`、`balance_infos` |
| Kimi | Coding Plan `booster_wallet`；开放平台 `GET /v1/users/me/balance` | 两者都是按量付费余额；当前只读 Coding Plan key 上的 `booster_wallet`，开放平台余额不和它混用 |
| Command Code | `credits.purchasedCredits + freeCredits` | 作为额外余额 |
| GLM | 无公开 API，只有控制台财务页 | 不读取，不进额度列 |
| MiniMax | 无公开 API，只有 Billing > Balance | 不读取，不进额度列 |
| OpenCode Go | 无 | 不读取 |

余额只在有公开接口且属于当前 key 可用范围时展示；不从控制台抓取，不把订阅扣费余额混进模型额度。

## 统一展示模型

协议和视图构建统一；共享额度保留独立呈现组件，因为它表达的是本地共享规则和结转，不是供应商套餐窗口：

```ts
type QuotaView =
  | { variant: "plan"; slots: RingSlot[]; primary: QuotaText; secondary?: QuotaText; stale: boolean; tooltip: QuotaTooltip }
  | { variant: "shared"; slots: RingSlot[]; primary: QuotaText; stale: boolean; tooltip: QuotaTooltip }
  | { variant: "balance"; balance: QuotaBalance; stale: boolean; tooltip: QuotaTooltip }
  | { variant: "status"; code: QuotaErrorCode | null; stale: boolean; tooltip: QuotaTooltip };
```

`buildQuotaView(response)` 是数据入口：

- `shared_limited` → `shared`，两个槽：当天、累积。
- 有套餐窗口 → `plan`，三个槽：rolling/day、week、month。
- 只有余额 → `balance`。
- 有错误或没有可用读数 → `status`。

- `QuotaCell` 渲染 `plan/balance/status`；`SharedQuotaCell` 渲染 `shared`。
- 两者共用 `QuotaRings`、`copy.ts`、`formatBalance` 和 `errorCopy`，不各自实现环、文案和余额格式化。
- `List` 只按 `mode` 选择单元格，不再理解窗口和余额字段。
- 供应商差异只存在于服务端适配器；共享额度的差异只存在于共享规则和结转，不扩散到其他供应商分支。

## 客户端展示

- `rolling`：按 span 显示“5 小时”或“N 小时/分钟”。
- 个人套餐：`week` → “7 天”，`month` → “月”，`custom` → “额度”；个人套餐不使用 `day`。
- 共享限额：`day` → “当天”，`week` → “本周累计”，`month` → “本月累计”。
- 圆环按 `kind` 分槽：`rolling`/`day` 内环，`week` 中环，`month` 外环；共享限额只有当天和累积两个槽。
- 个人套餐窗口只显示百分比；共享窗口显示剩余 token；余额按 `parts` 在客户端格式化。
- 窗口和余额同时存在时，窗口主读数是主角；余额作为次要文本，超宽省略，完整值放 tooltip。
- 视图模型统一处理空窗口、余额、错误和旧读数，不再由每个组件各自判断。

## 依据

- GLM 官方套餐概览：`https://docs.bigmodel.cn/cn/coding-plan/overview`，明确只有每 5 小时和每周额度。
- GLM 使用须知：`https://docs.bigmodel.cn/cn/coding-plan/usage-notes.md`，账户余额只用于订阅扣费。
- MiniMax M Plan 用量：`https://platform.minimax.io/docs/m-plan/usage-rules`，只有 5 小时和每周窗口，未用额度不结转。
- MiniMax 账户说明：`https://platform.minimax.io/docs/faq/about-account`，余额在 Billing 控制台，无公开查询接口。
- Kimi 余额接口：`https://platform.moonshot.cn/docs/api/balance`，返回 available/voucher/cash 余额。
- Kimi Coding Plan `usages` 与 `booster_wallet`：参考 `magpie/internal/provider/planquota.go:422-437`、`:538-557`、`:601-628`。
- GLM/MiniMax/Kimi 窗口参考：`cc-switch/src-tauri/src/services/coding_plan.rs:234-324`、`:671-735`、`:170-216`。
- Command Code 官方文档的 pricing/usage 页面当前返回 404，窗口和 credits 结构以管理 API 与上述两个参考项目为准。

## 发布与验证

- 协议先提交，服务端和客户端更新 submodule 指针后同批发布，不保留旧客户端兼容。
- 协议 serde 测试、适配器窗口测试、共享额度测试、预留键测试、客户端模型/文案和环位测试。

## 不在本次范围

- 共享额度 429 的错误码和参数（后续单独批次）。
- MCP 额度。
- 供应商共享用户管理的界面调整。

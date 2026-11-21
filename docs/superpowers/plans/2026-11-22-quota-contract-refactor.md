# 额度契约重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用一套语义化的额度模型替换展示字符串驱动的模型，让服务端只算数据、客户端只定文案。

**Architecture:** 先重建 `prelay-protocol` 的额度 DTO，再一次迁移服务端适配器、共享额度和预留，最后迁移客户端窗口模型和展示。三仓同批发布，不保留旧客户端兼容。

**Tech Stack:** Rust（`prelay-protocol`、`prelay-server`）、Nuxt/Vue/TypeScript（`prelay-client`）、Tauri。

**Spec:** `docs/superpowers/specs/2026-11-22-quota-contract-design.md`

## Global Constraints

- 不保留向后兼容；旧字段和旧解析路径直接删除。
- MCP 不进入协议；GLM 的 `TIME_LIMIT` 不再产出。
- 个人套餐窗口只传百分比；`remaining/limit/unit` 只用于共享限额的 token 余量。
- Command Code 的窗口只传百分比；额外 credits 只作为余额。
- 服务端不生成用户可见文案；`QuotaError.message` 只做诊断。
- 服务端全量 `cargo test --lib` 需要 `TEST_POSTGRES_URL`；本计划以定向测试和 clippy 为主，最后说明未覆盖边界。
- 每个仓库分别提交；submodule 指针和父仓改动一起提交；不推送、不发布。

## 前置状态

当前工作区已有一批未提交的共享额度改动：

- 客户端：`app/components/providers/List.vue`、`app/components/providers/quota/QuotaCell.vue`、`app/components/providers/sharing/RuleEditor.vue`、`app/components/providers/sharing/rule.ts`、`tests/provider-flow.test.ts`、`tests/quota-cell.test.ts`，以及新增的 `SharedQuotaCell.vue`、`SharedQuotaRings.vue`、`app/utils/provider/quota.ts`、`tests/shared-quota.test.ts`。
- 服务端：`src/store/storage/provider_quota.rs` 的共享窗口标签改动。

开始前需要用户授权把客户端这批改动提交为独立单元；服务端标签改动会被 Task 2 的契约迁移覆盖，不需要单独提交。未授权时保持未提交，并在 Task 1 前记录当前 `git status` 作为基线。

## File Structure

| 仓库 | 文件 | 责任 |
| --- | --- | --- |
| protocol | `src/providers.rs` | 额度 DTO 和窗口类型 |
| server | `src/service/quota/mod.rs` | 额度服务入口、缓存、分发 |
| server | `src/service/quota/adapter/*.rs` | 六个供应商适配器 |
| server | `src/service/quota/shared.rs` | 共享额度周期、结转、窗口组装 |
| server | `src/service/quota/reservation.rs` | 预留与窗口身份 |
| server | `src/store/storage/provider_quota.rs` | 共享规则读取、用量聚合 |
| client | `app/types/quota.ts` | 协议对应的 TS 类型 |
| client | `app/utils/provider/quota/model.ts` | 窗口选择、环位、排序 |
| client | `app/utils/provider/quota/copy.ts` | 窗口文案 |
| client | `app/components/providers/quota/*.vue` | 额度单元格与圆环 |
| client | `app/components/providers/List.vue` | 额度列入口 |

---

### Task 1: 重建协议额度模型

**Files:**
- Modify: `prelay-protocol/src/providers.rs:234-267`
- Test: `prelay-protocol/tests/management_dto.rs:560-620`

**Interfaces:**
- Produces: `QuotaWindowKind`、`QuotaUnit`、新的 `QuotaWindow`、新的 `QuotaBalance`。
- Later tasks rely on: `QuotaWindow { kind, span_secs, used_percent, resets_at, remaining, limit, unit, display }`。

- [ ] **Step 1: 写失败的协议测试**

```rust
#[test]
fn quota_windows_carry_semantic_kinds() {
    let window = QuotaWindow {
        kind: QuotaWindowKind::Month,
        span_secs: None,
        used_percent: 42.0,
        resets_at: Some("2026-12-01T00:00:00+08:00".into()),
        remaining: Some(58.0),
        limit: Some(100.0),
        unit: Some(QuotaUnit::Tokens),
        display: None,
    };
    let value = serde_json::to_value(&window).expect("serialize");
    assert_eq!(value["kind"], "month");
    assert_eq!(value["unit"], "tokens");
    assert!(value.get("label").is_none());
    assert!(value.get("aside").is_none());
    assert!(value.get("pool").is_none());
    let back: QuotaWindow = serde_json::from_value(value).expect("deserialize");
    assert_eq!(back, window);
}

#[test]
fn quota_windows_allow_percentage_only() {
    let value = serde_json::json!({
        "kind": "rolling",
        "span_secs": 18000,
        "used_percent": 12.5,
    });
    let window: QuotaWindow = serde_json::from_value(value).expect("deserialize");
    assert_eq!(window.kind, QuotaWindowKind::Rolling);
    assert!(window.remaining.is_none());
    assert!(window.limit.is_none());
    assert!(window.unit.is_none());
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --test management_dto quota_windows_carry_semantic_kinds`

Expected: 编译失败，`QuotaWindowKind`、`QuotaUnit` 不存在，`kind` 字段不存在。

- [ ] **Step 3: 实现新模型**

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
```

删除 `label`、`aside`、`pool`、`QuotaBalance.text`。更新 `management_dto.rs` 的额度 fixture。

- [ ] **Step 4: 运行协议测试**

Run: `cargo test`

Expected: 全部通过。

- [ ] **Step 5: 提交协议**

```text
git add src/providers.rs tests/management_dto.rs
git commit -m "重建额度窗口协议模型" -m "- 窗口改用 kind 和可选的数值字段
- 删除展示名称、旁路和池字段
- 余额改为只保留结构化币种和金额"
```

---

### Task 2: 迁移服务端额度契约

**Files:**
- Modify: `prelay-server/crates/protocol`（submodule 指针）
- Modify: `src/store/storage/provider_quota.rs`
- Modify: `src/service/quota/mod.rs`
- Modify: `src/service/quota/{deepseek,glm,kimi,minimax,commandcode,opencode}.rs`
- Modify: `src/service/quota/reservation.rs`
- Modify: `src/inbound/v1/candidates.rs`（测试夹具）
- Test: 各适配器内联测试、`src/service/quota/reservation/tests.rs`

**Interfaces:**
- Consumes: Task 1 的 `QuotaWindowKind`、`QuotaUnit`、`QuotaWindow`。
- Produces: 服务端所有窗口带 `kind`；共享窗口带 token 数值；预留键以 `kind` 为身份。

- [ ] **Step 0: 适配器补齐类型导入**

各适配器从 `prelay_protocol` 导入 `QuotaWindowKind`、`QuotaUnit`；测试模块通过 `use super::*` 使用这些类型。

- [ ] **Step 1: 把协议指针切到 Task 1 的提交**

```powershell
$sha = git -C ../prelay-protocol rev-parse HEAD
git -C crates/protocol checkout $sha
git add crates/protocol
```

- [ ] **Step 2: 写共享窗口测试**

```rust
#[test]
fn shared_windows_use_period_kinds() {
    assert_eq!(
        share_window_kind(ShareQuotaWindowKind::Day),
        QuotaWindowKind::Day
    );
    assert_eq!(
        share_window_kind(ShareQuotaWindowKind::Week),
        QuotaWindowKind::Week
    );
    assert_eq!(
        share_window_kind(ShareQuotaWindowKind::Month),
        QuotaWindowKind::Month
    );
}
```

- [ ] **Step 3: 更新共享窗口构造**

```rust
windows.push(QuotaWindow {
    kind: share_window_kind(rule.window),
    span_secs: match rule.window {
        ShareQuotaWindowKind::Day => Some(24 * 60 * 60),
        ShareQuotaWindowKind::Week => Some(7 * 24 * 60 * 60),
        ShareQuotaWindowKind::Month => None,
    },
    used_percent,
    resets_at: Some(end.to_rfc3339()),
    remaining: Some(remaining as f64),
    limit: Some(limit as f64),
    unit: Some(QuotaUnit::Tokens),
    display: None,
});
```

删除 `share_window_label`。

```rust
fn share_window_kind(kind: ShareQuotaWindowKind) -> QuotaWindowKind {
    match kind {
        ShareQuotaWindowKind::Day => QuotaWindowKind::Day,
        ShareQuotaWindowKind::Week => QuotaWindowKind::Week,
        ShareQuotaWindowKind::Month => QuotaWindowKind::Month,
    }
}
```

- [ ] **Step 4: 更新 GLM、MiniMax、Kimi**

GLM：

- 跳过 `kind == "TIME_LIMIT"` 的条目。
- `unit == 3` → `kind: Rolling`，`span_secs: n * 3600`。
- `unit == 6` → `kind: Week`，`span_secs: 7 * 86400`。
- 其他 → `kind: Custom`，`span_secs: None`。
- 不填 `remaining/limit/unit`，只填 `used_percent`、`resets_at`、`display`。

MiniMax：

- `ModelRemains` 增加 `current_weekly_status: i64`。
- 只取 `model_name == "general"` 的桶；视频/图像桶不进入模型调用额度。
- 5 小时窗口 → `kind: Rolling`，`span_secs: 18000`。
- 只有 `current_weekly_status == 1` 才产出周窗口；`status == 2` 时 `used_percent = 100.0`。
- 窗口不填数值字段。

Kimi：

- `limits[]` → `kind: Rolling`，`span_secs` 由 `window.duration` 和 `timeUnit` 计算。
- `usage` → `kind: Week`，`span_secs: 604800`。
- `usages.limit_5h` → `kind: Rolling`，`span_secs: 18000`。
- `usages.limit_7d` → `kind: Week`，`span_secs: 604800`。
- `usages.limit_month_total` 或没有 total 时的 `limit_month_code` → `kind: Month`，`span_secs: None`。
- `used_percent = used_ratio * 100`；同一跨度由 `usages` 覆盖 `limits/usage`。
- `booster_wallet.balance`：`type == "BOOSTER"` 时读 `amountLeft / 1_000_000 / 100` 作为按量付费余额，币种取 `monthlyChargeLimit.currency` 或 `monthlyUsed.currency`，缺省 `USD`。
- 不调用开放平台 `GET /v1/users/me/balance`；那是按量付费账户余额，和 Coding Plan key 不混用。

测试夹具必须覆盖：

```rust
#[test]
fn kimi_reads_month_and_booster() {
    let quota = parse(r#"{
        "usage": {"limit":"100","used":"12","resetTime":"2026-09-30T05:24:18Z"},
        "limits": [{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},
                    "detail":{"limit":"100","remaining":"88","resetTime":"2026-09-30T05:24:18Z"}}],
        "usages": {
            "limit_5h": {"used_ratio": 0.44, "reset_time": "2026-09-30T05:24:18Z"},
            "limit_7d": {"used_ratio": 0.18, "reset_time": "2026-10-02T00:00:00Z"},
            "limit_month_total": {"used_ratio": 0.31, "reset_time": "2026-10-01T00:00:00Z"}
        },
        "booster_wallet": {"balance": {"type": "BOOSTER", "amount": 100000000, "amountLeft": 25000000},
                           "monthlyChargeLimit": {"currency": "CNY"}}
    }"#).expect("kimi quota");
    assert_eq!(quota.windows.len(), 3);
    assert_eq!(quota.windows[2].kind, QuotaWindowKind::Month);
    assert_eq!(quota.windows[2].used_percent, 31.0);
    let balance = quota.balance.expect("booster");
    assert_eq!(balance.parts[0].currency, "CNY");
    assert_eq!(balance.parts[0].amount, "25.00");
}
```

```rust
#[test]
fn glm_skips_mcp_and_keeps_two_model_windows() {
    let quota = parse(r#"{
        "code": 200, "success": true,
        "data": {"level": "pro", "limits": [
            {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":12.5},
            {"type":"TOKENS_LIMIT","unit":6,"percentage":40},
            {"type":"TIME_LIMIT","unit":5,"percentage":3}
        ]}
    }"#).expect("glm quota");
    assert_eq!(quota.windows.len(), 2);
    assert_eq!(quota.windows[0].kind, QuotaWindowKind::Rolling);
    assert_eq!(quota.windows[1].kind, QuotaWindowKind::Week);
    assert!(quota.balance.is_none());
}

#[test]
fn minimax_skips_weekly_when_status_is_three() {
    let quota = parse_remains(r#"{
        "model_remains": [{
            "model_name": "general",
            "end_time": 1785182400000,
            "current_interval_remaining_percent": 99,
            "current_weekly_status": 3,
            "weekly_end_time": 1785715200000,
            "current_weekly_remaining_percent": 100
        }],
        "base_resp": {"status_code": 0}
    }"#).expect("minimax quota");
    assert_eq!(quota.windows.len(), 1);
    assert_eq!(quota.windows[0].kind, QuotaWindowKind::Rolling);
    assert!(quota.balance.is_none());
}

#[test]
fn deepseek_balance_keeps_structured_parts() {
    let quota = parse_balance(r#"{
        "is_available": true,
        "balance_infos": [{"currency":"CNY","total_balance":"110.00"}]
    }"#).expect("deepseek balance");
    let balance = quota.balance.expect("balance");
    assert_eq!(balance.parts[0].currency, "CNY");
    assert_eq!(balance.parts[0].amount, "110.00");
}
```

- [ ] **Step 5: 更新 DeepSeek、Command Code、OpenCode**

- DeepSeek：`QuotaBalance` 去掉 `text`，保留 `parts` 和 `available`。
- Command Code：三个窗口只填 `kind`、`span_secs`、`used_percent`、`resets_at`；不填 `remaining/limit/unit`。余额保留 `purchasedCredits + freeCredits`。
- OpenCode：`rolling/weekly/monthly` → `Rolling/Week/Month`；不填数值字段和 `unit`。
- GLM 和 MiniMax 不读取账户余额；OpenCode 没有余额。

测试：

```rust
#[test]
fn command_code_windows_are_percent_only() {
    let quota = parse(r#"{
        "credits": {"planId":"individual-goat","monthlyCredits":70,
                    "purchasedCredits":3,"freeCredits":1},
        "windowLimits": {
            "fiveHour": {"used":4,"cap":14,"resetAt":"2026-09-12T18:00:00Z"},
            "weekly": {"used":12,"cap":40,"resetAt":"2026-09-16T00:00:00Z"}
        }
    }"#, None).expect("command code");
    assert_eq!(quota.windows.len(), 3);
    assert!(quota.windows.iter().all(|window| window.remaining.is_none()));
    assert!(quota.windows.iter().all(|window| window.unit.is_none()));
    assert_eq!(quota.balance.expect("balance").parts[0].amount, "4.00");
}
```

- [ ] **Step 6: 更新预留身份**

```rust
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct HeldKey {
    provider_id: String,
    identity_id: String,
    window: QuotaWindowKind,
    reset: String,
}
```

剩余量从 `window.remaining.unwrap_or_default() as i64` 读取，不再解析字符串。

同步更新 `reservation/tests.rs` 和 `inbound/v1/candidates.rs` 里的窗口夹具：删除 `label/aside/amount` 旧字段，`limit` 改成新的可选数值字段，`unit` 使用 `QuotaUnit`。

测试：

```rust
#[test]
fn held_keys_distinguish_window_kinds() {
    let day = HeldKey {
        provider_id: "p".into(),
        identity_id: "i".into(),
        window: QuotaWindowKind::Day,
        reset: "r".into(),
    };
    let week = HeldKey { window: QuotaWindowKind::Week, ..day.clone() };
    assert_ne!(day, week);
}
```

- [ ] **Step 7: 运行服务端定向验证**

Run:

```text
cargo fmt --all
cargo test --lib service::quota
cargo test --lib store::storage::provider_quota
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: 全部通过。全量 `cargo test --lib` 若缺少 `TEST_POSTGRES_URL`，记录 86 个数据库用例的环境失败。

- [ ] **Step 8: 提交服务端契约迁移**

```text
git add crates/protocol src
git commit -m "迁移额度窗口到语义模型" -m "- 适配器按窗口类型输出，不再拼展示文案
- 共享额度只保留 token 余量，个人套餐只保留百分比
- Kimi 补月窗口和加油包余额，MiniMax 按周状态过滤
- 预留以窗口类型和重置时间作为身份"
```

---

### Task 3: 拆分额度服务结构

**Files:**
- Create: `src/service/quota/shared.rs`
- Move: `src/service/quota/{deepseek,glm,kimi,minimax,commandcode,opencode}.rs` → `src/service/quota/adapter/`
- Modify: `src/service/quota/mod.rs`
- Modify: `src/store/storage/provider_quota.rs`

- [ ] **Step 1: 建立结构门禁测试**

在 `src/service/quota/mod.rs` 增加：

```rust
#[cfg(test)]
mod structure_tests {
    #[test]
    fn quota_adapters_live_under_adapter_module() {
        assert!(std::path::Path::new("src/service/quota/adapter/glm.rs").exists());
        assert!(std::path::Path::new("src/service/quota/shared.rs").exists());
        assert!(!std::path::Path::new("src/service/quota/glm.rs").exists());
    }
}
```

Run: `cargo test --lib structure_tests`；Expected: 失败。

- [ ] **Step 2: 移动适配器**

```text
git mv src/service/quota/deepseek.rs src/service/quota/adapter/deepseek.rs
git mv src/service/quota/glm.rs src/service/quota/adapter/glm.rs
git mv src/service/quota/kimi.rs src/service/quota/adapter/kimi.rs
git mv src/service/quota/minimax.rs src/service/quota/adapter/minimax.rs
git mv src/service/quota/commandcode.rs src/service/quota/adapter/commandcode.rs
git mv src/service/quota/opencode.rs src/service/quota/adapter/opencode.rs
```

创建 `adapter/mod.rs` 声明子模块；`mod.rs` 改为 `mod adapter;`，分发路径改为 `adapter::glm::fetch`。
移动后把适配器内的 `super::{...}` 引用改成 `super::super::{...}` 或 `crate::service::quota::{...}`。

- [ ] **Step 3: 移动共享周期数学**

- `store/storage/provider_quota.rs` 保留 `shared_quota_rule` 和 `used_tokens`，改成供服务层调用的窄接口。
- 新建 `service/quota/shared.rs`，迁入 `share_window_bounds`、`previous_window_bounds`、结转和 `QuotaWindow` 组装。
- `mod.rs` 和 `reservation.rs` 改为调用 `shared::windows(...)`。

- [ ] **Step 4: 运行结构验证**

Run:

```text
cargo test --lib structure_tests
cargo test --lib service::quota
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: 通过，旧路径不存在。

- [ ] **Step 5: 提交结构拆分**

```text
git add src
git commit -m "拆分额度服务职责" -m "- 适配器收拢到 adapter 目录
- 共享额度的周期和结转移出存储层
- 存储层只保留规则读取和用量聚合"
```

---

### Task 4: 迁移客户端额度模型与展示

**Files:**
- Modify: `prelay-client/crates/protocol`（submodule 指针）
- Modify: `app/types/quota.ts`
- Create: `app/utils/provider/quota/model.ts`
- Create: `app/utils/provider/quota/copy.ts`
- Create: `app/utils/provider/quota/view.ts`
- Modify: `app/utils/provider/quota.ts`
- Create: `app/components/providers/quota/QuotaRings.vue`
- Modify: `app/components/providers/quota/QuotaCell.vue`
- Modify: `app/components/providers/quota/SharedQuotaCell.vue`
- Delete: `app/components/providers/quota/{PlanRings,SharedQuotaRings}.vue`
- Modify: `app/components/providers/List.vue`
- Test: `tests/quota-model.test.ts`、`tests/quota-cell.test.ts`、`tests/shared-quota.test.ts`

**Interfaces:**
- Consumes: Task 1 的 JSON 字段。
- Produces: `buildQuotaView(response)`、`ringSlots(windows, variant)`、`windowCopy(window, mode)`、`formatBalance(parts)`、`errorCopy(code)`。

- [ ] **Step 0: 切客户端协议指针**

```powershell
$sha = git -C ../prelay-protocol rev-parse HEAD
git -C crates/protocol checkout $sha
```

- [ ] **Step 1: 写失败的模型测试**

```ts
import { expect, test } from "bun:test";
import type {
  ProviderQuota,
  ProviderQuotaMode,
  ProviderQuotaResponse,
  QuotaWindow,
} from "~/types/quota";
import { errorCopy, windowCopy, windowSlot } from "~/utils/provider/quota/copy";
import { tightestWindow } from "~/utils/provider/quota/model";
import { buildQuotaView } from "~/utils/provider/quota/view";

const window = (patch: Partial<QuotaWindow>): QuotaWindow => ({
  kind: "rolling",
  span_secs: 18000,
  used_percent: 10,
  resets_at: null,
  remaining: null,
  limit: null,
  unit: null,
  display: null,
  ...patch,
});

test("窗口文案由类型和跨度决定", () => {
  expect(windowCopy(window({}), "own")).toBe("5 小时");
  expect(windowCopy(window({ kind: "day", span_secs: 86400 }), "shared_limited")).toBe("当天");
  expect(windowCopy(window({ kind: "week", span_secs: 604800 }), "shared_limited")).toBe("本周累计");
  expect(windowCopy(window({ kind: "month", span_secs: null }), "own")).toBe("月");
});

test("圆环按窗口类型分槽", () => {
  expect(windowSlot(window({}), "own")).toBe(0);
  expect(windowSlot(window({ kind: "day" }), "shared_limited")).toBe(0);
  expect(windowSlot(window({ kind: "week" }), "own")).toBe(1);
  expect(windowSlot(window({ kind: "month" }), "own")).toBe(2);
});

test("最紧窗口按剩余比例取最小", () => {
  const day = window({ kind: "day", used_percent: 5 });
  const week = window({ kind: "week", used_percent: 40 });
  expect(tightestWindow([day, week])?.kind).toBe("week");
});

test("错误文案由 code 决定", () => {
  expect(errorCopy("unreachable")).toBe("无法连接");
  expect(errorCopy("unconfigured")).toBe("待配置");
});

const response = (
  mode: ProviderQuotaMode,
  quota: Partial<ProviderQuota>,
): ProviderQuotaResponse => ({
  provider_id: "p",
  owner_identity_id: "o",
  owner_display_name: "owner",
  mode,
  quota: { windows: [], balance: null, error: null, stale: false, ...quota },
  updated_at: null,
});

test("视图模型统一 plan/shared/balance/status", () => {
  const plan = buildQuotaView(
    response("own", {
      windows: [
        window({ kind: "rolling" }),
        window({ kind: "week" }),
        window({ kind: "month" }),
      ],
    }),
  );
  expect(plan.variant).toBe("plan");
  expect(plan.slots).toHaveLength(3);

  const shared = buildQuotaView(
    response("shared_limited", {
      windows: [
        window({ kind: "day", remaining: 100, limit: 200, unit: "tokens" }),
        window({ kind: "week", remaining: 500, limit: 1000, unit: "tokens" }),
      ],
    }),
  );
  expect(shared.variant).toBe("shared");
  expect(shared.slots).toHaveLength(2);

  const balance = buildQuotaView(
    response("own", {
      balance: { parts: [{ currency: "USD", amount: "5.00" }], available: true },
    }),
  );
  expect(balance.variant).toBe("balance");

  const status = buildQuotaView(
    response("own", { error: { code: "unreachable", message: "x" } }),
  );
  expect(status.variant).toBe("status");
});
```

- [ ] **Step 2: 运行确认失败**

Run: `bun test tests/quota-model.test.ts`

Expected: 模块不存在。

- [ ] **Step 3: 实现类型和模型**

`app/types/quota.ts` 换成协议字段：

```ts
export type QuotaWindowKind = "custom" | "rolling" | "day" | "week" | "month";
export type QuotaUnit = "tokens" | "usd" | "credits" | "requests";

export interface QuotaWindow {
  kind: QuotaWindowKind;
  span_secs?: number | null;
  used_percent: number;
  resets_at?: string | null;
  remaining?: number | null;
  limit?: number | null;
  unit?: QuotaUnit | null;
  display?: string | null;
}
```

`copy.ts`：

```ts
import type {
  ProviderQuotaMode,
  QuotaBalancePart,
  QuotaErrorCode,
  QuotaWindow,
} from "~/types/quota";

function currencySymbol(currency: string) {
  if (currency === "CNY") return "¥";
  if (currency === "USD") return "$";
  return "";
}

export function formatBalance(parts: QuotaBalancePart[]) {
  return parts
    .map((part) => `${currencySymbol(part.currency)}${part.amount}`)
    .join(" · ");
}

export function errorCopy(code: QuotaErrorCode) {
  switch (code) {
    case "unsupported":
      return "未接入";
    case "unconfigured":
      return "待配置";
    case "unauthorized":
      return "授权失败";
    case "unreachable":
      return "无法连接";
    case "invalid_response":
      return "返回异常";
  }
}

function formatSpan(span: number | null | undefined) {
  if (!span) return "额度";
  if (span % 86400 === 0) return `${span / 86400} 天`;
  if (span % 3600 === 0) return `${span / 3600} 小时`;
  return `${Math.round(span / 60)} 分钟`;
}

export function windowCopy(window: QuotaWindow, mode: ProviderQuotaMode) {
  if (window.kind === "rolling") return formatSpan(window.span_secs);
  if (window.kind === "day") return mode === "shared_limited" ? "当天" : "1 天";
  if (window.kind === "week") return mode === "shared_limited" ? "本周累计" : "7 天";
  if (window.kind === "month") return mode === "shared_limited" ? "本月累计" : "月";
  return "额度";
}

export function windowSlot(window: QuotaWindow, mode: ProviderQuotaMode) {
  if (mode === "shared_limited") return window.kind === "day" ? 0 : 1;
  if (window.kind === "rolling" || window.kind === "day") return 0;
  if (window.kind === "week") return 1;
  return 2;
}
```

`model.ts`：`remainingPercent`、`tightestWindow`、`sortWindows`、`hasWindowAmount`。

- [ ] **Step 4: 更新组件**

- 新建 `QuotaRings`，只接收 `slots`；`QuotaCell` 和 `SharedQuotaCell` 共用它。
- `QuotaCell` 渲染 `plan/balance/status`；`SharedQuotaCell` 渲染 `shared`，保持独立组件。
- 两者共用 `windowCopy`、`formatBalance`、`errorCopy`；个人窗口只显示百分比，共享窗口显示 token 余量。
- `List.vue` 只按 `mode` 选择单元格，不再理解窗口和余额字段。
- 删除 `aside` 过滤、`unitOrder`、`label` 解析和 `balance.text` 使用。
- 余额由 `formatBalance(parts)` 生成 `¥128.40 · $5.00`，不再读服务端拼接文本。
- 窗口和余额同时存在时，窗口主读数优先，余额作为次要文本并允许省略。
- 窗口与余额不合并；余额是按量付费/预付余额，`tightestWindow` 只看窗口。
- 错误态用 `errorCopy(code)`，不再展示 `QuotaError.message`。

- [ ] **Step 5: 更新测试和验证**

- `tests/quota-cell.test.ts` 里的 `QuotaPlanRings` 断言改为 `QuotaRings`。
- `tests/shared-quota.test.ts` 里的 `SharedQuotaRings` 断言改为 `QuotaRings`，共享文案断言保留。

Run:

```text
bun test tests/quota-model.test.ts tests/quota-cell.test.ts tests/shared-quota.test.ts
bunx eslint app/components/providers app/utils/provider tests
bunx prettier --check app tests
bun run typecheck
```

Expected: 全部通过。

- [ ] **Step 6: 提交客户端**

```text
git add crates/protocol app tests
git commit -m "重建额度列展示模型" -m "- 窗口文案和环位改由 kind 决定
- 共享额度只展示当天和累积，个人套餐只展示百分比
- 删除按中文标签解析、aside 过滤和预格式化余额文本"
```

---

### Task 5: 发布前核对

**Files:**
- Modify: `docs/superpowers/specs/2026-11-17-quota-design.md`（标注被本设计取代）

- [ ] **Step 0: 标注旧设计**

在 `2026-11-17-quota-design.md` 顶部加一行：

```text
> 2026-11-22 起，额度协议与展示以 `2026-11-22-quota-contract-design.md` 为准。
```

- [ ] **Step 1: 检查旧字段是否清零**

Run:

```text
rg -n "\.label|\.aside|\.pool|balance\.text" ../prelay-server/src/service/quota ../prelay-server/src/store/storage/provider_quota.rs ../prelay-client/app/components/providers/quota ../prelay-client/app/types/quota.ts ../prelay-protocol/src/providers.rs
```

Expected: 只命中 `ShareQuotaWindow` 或非额度字段；额度窗口不再出现这些字段。

- [ ] **Step 2: 三仓验证**

Run:

```text
cd ../prelay-protocol && cargo test
cd ../prelay-server && cargo test --lib service::quota && cargo clippy --all-targets --all-features -- -D warnings
cd ../prelay-client && bun test && bun run typecheck && bunx eslint .
```

Expected: 通过；服务端全量库测试如缺少 `TEST_POSTGRES_URL`，单独记录。

- [ ] **Step 3: 提交旧设计标注**

```powershell
git add docs/superpowers/specs/2026-11-17-quota-design.md
git commit -m "标注额度旧设计已取代" -m "- 新协议和展示以 2026-11-22 设计为准"
```

- [ ] **Step 4: 报告发布边界**

说明服务器和客户端必须同批升级、旧客户端不再兼容；版本号、推送和发布另行授权，不在本计划内执行。

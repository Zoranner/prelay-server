# 供应商额度 V2 与共享额度实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把现有额度 v1 重构成统一的额度卡片模型，按供应商目录声明取数方式，补齐 6 家供应商适配器，并为共享额度补上并发预留。

**Architecture:** 协议定义一张卡（窗口和余额可以同时存在）；服务端按目录的 `[providers.quota]` 选择命名适配器，并发取数并保留上次成功读数；共享额度继续走本地活动记录，但请求前增加预留、结束后结算。

**Tech Stack:** Rust、Axum、SeaORM、PostgreSQL、Vue 3、Nuxt、Tauri、Bun

**Spec:** `docs/superpowers/specs/2026-11-16-quota-sharing-design.md`（v1；Task 1 会替换其中的额度模型）

**参考实现:** `.refs/magpie` 的 `internal/provider/quotas.go`、`planquota.go`、`balance.go`、`quota_last.go`、`internal/budget/budget.go`

## Global Constraints

- 不新增独立额度页面；额度只出现在供应商列表的一列和浮层里
- 套餐和余额不允许手填；没有接口就显示“未接入”，没有配置就显示“待配置”
- 共享设置与额度查看分开；创建者查看被共享人用量暂不实现
- 三套展示：套餐窗口、余额、共享限额
- 千问 Token Plan 已退役；GoToken 没有公开额度接口，保持未接入
- `prelay-protocol` 是 DTO 和错误码的唯一来源；不在两个 submodule 各改一份
- 提交、推送、发布分别授权；本计划不自动授权任何远端操作
- 当前工作树同时有“额度 v1”和“记录日志/异步物化”两组未提交改动，并行前必须先隔离
- 同一时间只允许一个会话修改 `prelay-server/src/service/quota/mod.rs`

---

## 进度

- [x] Task 0：额度 v1 与千问退役已提交；记录日志改动仍留在 `master` 工作树；额度 v2 使用独立 worktree `prelay-server-quota-v2`
- [x] Task 1：协议卡片模型 `057d877`、服务端 `5403f3d`、客户端 `d22297d`
- [ ] Task 2：目录 `[providers.quota]` 与适配器注册
- [ ] Task 3：GLM、Kimi、Command Code、OpenCode Go 适配器
- [ ] Task 4：并发取数、缓存与错误状态
- [ ] Task 5：共享额度预留
- [ ] Task 6：客户端新模型收尾
- [ ] Task 7：全量收口

---

## 当前状态

已完成但未提交：

- 协议 v1 额度 DTO：`ProviderQuota` 枚举、`QuotaWindowResponse`、共享额度窗口
- 共享额度存储：`identity_provider_shares.quota_json`
- 服务端额度服务：DeepSeek 余额、MiniMax 5 小时/周
- `GET /api/providers/quotas`
- 客户端额度列、三环、余额、共享进度条、共享抽屉
- 千问 Token Plan 目录退役（3 个文件，测试已通过）

需要返工或新增：

- 协议从互斥枚举改成一张卡；窗口改成自由形态
- 目录增加 `[providers.quota]` 和适配器注册表
- 补齐 GLM、Kimi、Command Code、OpenCode Go；重构 DeepSeek、MiniMax
- 并发取数、单家超时、缓存、上次成功读数、错误分级
- 共享额度原子预留
- 客户端适配新模型和四种状态

明确不做：

- 千问 Token Plan 登录流程
- GoToken 额度查询
- 手填额度、独立额度页面、趋势图、创建者查看被共享人用量

## 并行边界

| 文件或目录 | 归属 |
| --- | --- |
| `prelay-protocol/src/providers.rs` 及两个 submodule | 只能 Task 1 所在会话 |
| `prelay-server/src/service/quota/mod.rs` | 同一时间只能一个会话 |
| `prelay-server/src/store/storage/provider_quota.rs` | Task 4 或 Task 5，二选一 |
| `prelay-server/config/catalog/providers.toml` | Task 2；适配器任务不写这里 |
| `src/lib.rs`、`store/mod.rs`、`storage/mod.rs`、`bootstrap/serve.rs` | 与记录日志改动重叠，Task 0 先隔离 |
| `prelay-client/app/pages/providers.vue`、`List.vue` | 只能 Task 6 |
| `Cargo.toml` / `Cargo.lock` | 本计划不新增依赖 |

---

### Task 0: 基线隔离与提交边界

**Files:**
- 无代码文件；只处理 Git 工作树、提交和 worktree

**Interfaces:**
- Consumes: 当前 49 个未提交文件
- Produces: 一个干净的基线提交或独立 worktree，供后续并行会话使用

- [ ] **Step 1: 确认两组改动**

Run:

```powershell
git -C prelay-server status --short
git -C prelay-client status --short
git -C prelay-protocol status --short
```

Expected: 额度 v1 与记录日志改动都能从文件清单中分开；`prelay-protocol` 和两个 submodule 都有协议改动。

- [ ] **Step 2: 先提交千问退役**

只暂存：

```powershell
git -C prelay-server add -- config/catalog/providers.toml src/store/schema/provider_catalog.rs tests/provider_catalog.rs
git -C prelay-server diff --cached --stat
git -C prelay-server diff --cached --check
```

提交信息：

```text
删除千问 Token Plan 供应商配置

- qwen 与 qwen_token_plan 不再映射到目录条目
- qwen3.8-max 和 qwen3.8-flash 模型保留给 Command Code 与 OpenCode Go
- 增加供应商退役回归断言
```

Expected: 3 个文件，1 个提交；其他未提交文件保持原状。

- [ ] **Step 3: 决定额度 v1 与记录日志的提交顺序**

先跑两组各自的测试，选先完成且测试通过的一组提交。额度 v1 涉及的文件以 `quota`、`provider_shares`、`providers.vue` 为主；记录日志涉及 `recording_journal`、`rollout`、`recording_timing`。

Expected: 两组改动分别有清晰的提交边界；不能把两组混进同一个提交。

- [ ] **Step 4: 为并行会话创建 worktree**

在基线提交之后：

```powershell
git -C prelay-server worktree add ..\prelay-server-quota-v2 -b feat/quota-v2
git -C prelay-client worktree add ..\prelay-client-quota-v2 -b feat/quota-v2
git -C prelay-protocol worktree add ..\prelay-protocol-quota-v2 -b feat/quota-v2
```

Expected: 每个会话有独立目录；协议会话只在自己的 worktree 改协议。

---

### Task 1: 协议卡片模型

**Files:**
- Modify: `prelay-protocol/src/providers.rs`
- Modify: `prelay-protocol/src/lib.rs`
- Test: `prelay-protocol/tests/management_dto.rs`
- Sync: `prelay-server/crates/protocol`、`prelay-client/crates/protocol`

**Interfaces:**
- Consumes: 现有 `ProviderQuotaMode`、`ShareQuotaWindow`
- Produces: `ProviderQuota` 卡片、`QuotaWindow`、`QuotaBalance`、`QuotaError`、`QuotaErrorCode`

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaWindow {
    pub label: String,
    pub span_secs: Option<i64>,
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub display: Option<String>,
    pub amount: Option<String>,
    pub limit: Option<String>,
    pub unit: Option<String>,
    pub aside: bool,
    pub pool: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaBalancePart {
    pub currency: String,
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaBalance {
    pub text: String,
    pub parts: Vec<QuotaBalancePart>,
    pub available: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuotaErrorCode {
    Unsupported,
    Unconfigured,
    Unauthorized,
    Unreachable,
    InvalidResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaError {
    pub code: QuotaErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderQuota {
    pub plan: Option<String>,
    pub windows: Vec<QuotaWindow>,
    pub balance: Option<QuotaBalance>,
    pub error: Option<QuotaError>,
    pub read_at: Option<String>,
    pub stale: bool,
}
```

- [ ] **Step 1: 写序列化失败测试**

在 `prelay-protocol/tests/management_dto.rs` 增加一个用例，要求同一个 `ProviderQuota` 同时带 `windows` 和 `balance`，并断言枚举形态的 `kind` 不再出现。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p prelay-protocol management_dto`

Expected: 新字段不存在，测试失败。

- [ ] **Step 3: 替换协议类型**

删除 `ProviderQuota` 枚举、`QuotaWindowResponse`、`QuotaBalanceResponse`；换成上面的结构。`ProviderQuotaResponse` 保留，`quota` 字段改成 `ProviderQuota`。

- [ ] **Step 4: 更新共享窗口类型**

把共享窗口的 `QuotaWindowKind` 改名为 `ShareQuotaWindowKind`，只允许 `day`、`week`、`month`，避免和供应商窗口混淆。

- [ ] **Step 5: 运行协议测试**

Run: `cargo test -p prelay-protocol`

Expected: 全部通过。

- [ ] **Step 6: 同步两个 submodule**

提交协议后，在 `prelay-server` 和 `prelay-client` 分别更新 `crates/protocol` 指针。协议提交与父仓指针更新分开提交。

---

### Task 2: 目录额度声明与适配器注册

**Files:**
- Modify: `prelay-server/src/domain/catalog/mod.rs`
- Modify: `prelay-server/src/domain/catalog/loading.rs`
- Modify: `prelay-server/src/domain/catalog/validation.rs`
- Modify: `prelay-server/config/catalog/providers.toml`
- Modify: `prelay-server/src/service/quota/mod.rs`
- Test: `prelay-server/tests/provider_catalog.rs`

**Interfaces:**
- Consumes: `ProviderQuota`、`QuotaWindow`
- Produces: `CatalogQuota { adapter, url, auth, timeout_secs, requires }` 和 `QuotaAdapterKind`

目录示例：

```toml
[providers.quota]
adapter = "commandcode_credits"
url = "https://api.commandcode.ai/alpha/billing/credits"
auth = "bearer"
timeout_secs = 8
```

```toml
[providers.quota]
adapter = "glm_coding_plan"
url = "https://open.bigmodel.cn/api/monitor/usage/quota/limit"
auth = "raw"
timeout_secs = 8
requires = ["plan_type"]
```

- [ ] **Step 1: 写目录校验失败测试**

在 `tests/provider_catalog.rs` 增加两个用例：未知 `adapter` 必须加载失败；`auth = "raw"` 必须能加载。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p prelay-server --test provider_catalog`

Expected: `quota` 字段不存在，测试失败。

- [ ] **Step 3: 增加目录结构**

`RawProvider` 增加 `quota: Option<RawProviderQuota>`；`CatalogProvider` 增加 `quota: Option<CatalogQuota>`。`url` 允许绝对地址，也允许 `/path` 相对 `base_url` 的 origin。

- [ ] **Step 4: 增加适配器枚举**

```rust
pub enum QuotaAdapterKind {
    DeepSeekBalance,
    GlmCodingPlan,
    KimiCodeUsages,
    MiniMaxPlan,
    CommandCodeCredits,
    OpenCodeGoUsage,
}
```

`from_catalog` 只接受上面 6 个名字；未知名字在目录加载时直接报错，不等到请求时才失败。

- [ ] **Step 5: 写入 6 家目录声明**

按供应商逐一增加 `[providers.quota]`，不写密钥、不写账号上下文。

- [ ] **Step 6: 运行目录测试**

Run: `cargo test -p prelay-server --test provider_catalog`

Expected: 全部通过；未知适配器被拒绝。

---

### Task 3: 供应商适配器

可以拆成两路并行；只允许改各自的适配器文件，不改 `mod.rs` 注册表。

**Files:**
- Create: `prelay-server/src/service/quota/adapters/deepseek.rs`
- Create: `prelay-server/src/service/quota/adapters/glm.rs`
- Create: `prelay-server/src/service/quota/adapters/kimi.rs`
- Create: `prelay-server/src/service/quota/adapters/minimax.rs`
- Create: `prelay-server/src/service/quota/adapters/commandcode.rs`
- Create: `prelay-server/src/service/quota/adapters/opencode.rs`
- Modify: `prelay-server/src/service/quota/mod.rs`（只由集成会话接注册）

**Interfaces:**
- Consumes: `CatalogQuota`、解密后的 API Key、`reqwest::Client`
- Produces: `QuotaReading { plan, windows, balance }` 或 `QuotaError`

| 适配器 | 端点 | 关键字段 | 窗口语义 |
| --- | --- | --- | --- |
| `deepseek_balance` | `GET https://api.deepseek.com/user/balance` | `is_available`、`balance_infos[]` | 无窗口，余额 |
| `glm_coding_plan` | `GET /api/monitor/usage/quota/limit` | `data.limits[]`：`type`、`unit`、`number`、`percentage`、`nextResetTime` | `unit=3` 5 小时，`unit=6` 周；`percentage` 已用 |
| `kimi_code_usages` | `GET https://api.kimi.com/coding/v1/usages` | `usage`、`limits[]`、`remaining`、`resetTime` | 周 + 动态短窗口；数字可能是字符串 |
| `minimax_plan` | `GET /v1/token_plan/remains` | `model_remains[]`、`base_resp` | 5 小时 + 周；`remaining_percent` 是剩余；status 2/3 特殊 |
| `commandcode_credits` | `GET https://api.commandcode.ai/alpha/billing/credits` | `windowLimits.fiveHour/weekly`、`credits` | 5 小时 + 周 + 月度 credits |
| `opencode_go_usage` | `GET https://opencode.ai/zen/go/v1/usage` | `usage.rolling/weekly/monthly` | 5 小时 + 周 + 月；`percent` 已用 |

- [ ] **Step 1: 每个适配器先写 fixture 测试**

每个文件至少覆盖：正常响应、信封失败、字段缺失、百分比方向。测试只调用纯解析函数，不发真实请求。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p prelay-server --lib service::quota::adapters`

Expected: 模块不存在，测试失败。

- [ ] **Step 3: 实现解析**

统一输出 `used_percent`，0 到 100；绝对量用十进制字符串；金额用 `QuotaBalance.text`。信封先判状态，再取字段。无法识别的字段返回 `InvalidResponse`，不猜。

- [ ] **Step 4: 运行适配器测试**

Run: `cargo test -p prelay-server --lib service::quota::adapters`

Expected: 6 个适配器全部通过。

---

### Task 4: 取数、缓存与错误状态

**Files:**
- Modify: `prelay-server/src/service/quota/mod.rs`
- Create: `prelay-server/src/service/quota/cache.rs`
- Modify: `prelay-server/src/store/storage/provider_quota.rs`
- Modify: `prelay-server/src/inbound/api/providers.rs`

**Interfaces:**
- Consumes: `QuotaAdapterKind`、`CatalogQuota`、`ProviderQuota`
- Produces: `QuotaService::list_for_identity`，带并发、缓存、上次成功读数和错误码

- [ ] **Step 1: 写并发与超时测试**

用注入的 `reqwest::Client` 或本地 mock server，模拟 6 家供应商各延迟 2 秒；断言总耗时小于 5 秒，而不是 12 秒。

- [ ] **Step 2: 写缓存与 stale 测试**

第一次成功返回 `¥10.00`，第二次上游 503；断言仍返回 `¥10.00`，`stale=true`，`read_at` 保留第一次时间。

- [ ] **Step 3: 实现并发取数**

用 `futures::future::join_all`；每家 4 秒超时；整体 6 秒预算；超预算的供应商返回缓存或 `stale`。

- [ ] **Step 4: 实现缓存键**

键包含 `provider_id`、`adapter`、`url`、API Key 摘要、`requires` 上下文摘要。成功缓存 60 秒，错误缓存 15 秒。

- [ ] **Step 5: 实现错误分级**

- 401/403：`Unauthorized`
- 超时、连接失败、5xx：`Unreachable`
- JSON 或字段不符：`InvalidResponse`
- 缺 `requires`：`Unconfigured`
- 没有目录声明：`Unsupported`

- [ ] **Step 6: 运行额度服务测试**

Run: `cargo test -p prelay-server --lib service::quota`

Expected: 并发、缓存、错误分级全部通过。

---

### Task 5: 共享额度预留

**Files:**
- Create: `prelay-server/src/service/quota/reservation.rs`
- Modify: `prelay-server/src/store/storage/provider_quota.rs`
- Modify: `prelay-server/src/store/storage/access.rs`
- Test: `prelay-server/tests/management/provider_sharing.rs`

**Interfaces:**
- Consumes: 现有 `ShareQuotaWindow`、`shared_quota_windows`
- Produces: `ReservationGuard { settle(actual_tokens) }`、`QuotaRefusal { window, used, limit, resets_at, retry_after_secs }`

- [ ] **Step 1: 写并发超卖测试**

共享限额 1000 tokens，已用 900；同时发 20 个请求，每个预留 100。断言最多 1 个进入，其余收到拒绝。

- [ ] **Step 2: 写结算测试**

预留 100，实际使用 40；结算后下一次请求看到的已用是 940，不是 1000。

- [ ] **Step 3: 实现预留**

按 `(provider_id, grantee_identity_id, window_start)` 维护内存 `held`；预留量 = 请求体字节数 ÷ 4 + 本窗口平均输出。预留前检查所有启用窗口，任一不足即拒绝。

- [ ] **Step 4: 实现结算**

请求结束后释放预留；如果活动记录是异步物化，结算值先进入短期 `recent_usage` 覆盖层，等活动记录可见后移除。这个依赖必须在实现前确认；如果记录日志改动尚未合并，先只做进程内预留并标注边界。

- [ ] **Step 5: 实现拒绝响应**

当前 `access.rs` 在额度耗尽时直接跳过该供应商；新实现要在候选解析阶段预留，所有候选都耗尽时返回带最紧窗口、已用、上限、重置时间和 `Retry-After` 的额度拒绝，不返回 500，也不伪装成“模型不存在”。

- [ ] **Step 6: 运行共享测试**

Run: `cargo test -p prelay-server --test management provider_sharing`

Expected: 并发、结算、拒绝全部通过。

---

### Task 6: 客户端额度列

**Files:**
- Modify: `prelay-client/app/types/quota.ts`
- Modify: `prelay-client/app/composables/command/relay.ts`
- Modify: `prelay-client/app/components/providers/quota/QuotaCell.vue`
- Modify: `prelay-client/app/components/providers/quota/PlanRings.vue`
- Modify: `prelay-client/app/components/providers/List.vue`
- Modify: `prelay-client/app/pages/providers.vue`
- Test: `prelay-client/tests/quota-cell.test.ts`
- Test: `prelay-client/tests/provider-flow.test.ts`

**Interfaces:**
- Consumes: 新 `ProviderQuota` DTO
- Produces: 额度列的四种状态和“窗口 + 余额”同时展示

- [ ] **Step 1: 更新类型和命令**

让 Tauri command 返回新 DTO；删掉旧的枚举分支。

- [ ] **Step 2: 写渲染测试**

覆盖：只有窗口、只有余额、两者都有、`unsupported`、`unconfigured`、`unreachable`、`stale`。

- [ ] **Step 3: 实现额度列**

窗口画环，按最紧窗口排序；余额显示格式化文本；两者都有时环为主、余额为副；错误状态用灰字，不伪造 0。

- [ ] **Step 4: 保持紧凑**

列宽不增加；浮层展示全部窗口、重置时间、余额明细和 `read_at`。

- [ ] **Step 5: 运行客户端检查**

Run:

```powershell
bun run typecheck
bun test tests/quota-cell.test.ts tests/provider-flow.test.ts
```

Expected: 全部通过。

---

### Task 7: 收口验证

- [ ] **Step 1: 协议**

Run: `cargo test -p prelay-protocol`

- [ ] **Step 2: 服务端**

Run:

```powershell
cargo fmt -p prelay-server -- --check
cargo clippy -p prelay-server --all-targets --all-features -- -D warnings
cargo test -p prelay-server --all-features
```

- [ ] **Step 3: 客户端**

Run:

```powershell
bun run format:check
bun run lint
bun run typecheck
bun test
```

- [ ] **Step 4: 记录未覆盖边界**

- 没有生产密钥，真实上游额度接口未验证
- GoToken 仍为未接入
- 千问已退役，历史行的删除行为未做端到端验证
- 如果记录日志改动未合并，共享预留的异步结算只做进程内版本

---

## 并行执行方案

### 推荐：3 个会话

| 会话 | 负责 | 开始条件 |
| --- | --- | --- |
| S1 主线 | Task 0 → 1 → 4 → 5 → 7 | Task 0 完成 |
| S2 适配器 | Task 3 的 6 个适配器 | Task 1、2 完成 |
| S3 客户端 | Task 6 | Task 1 完成 |

S1 在 Task 1 完成前不能并行；Task 1 一落地，S2、S3 同时开始。

### 激进：4 个会话

| 会话 | 负责 |
| --- | --- |
| S1 | Task 0 → 1 |
| S2 | Task 3a：DeepSeek、Command Code、MiniMax |
| S3 | Task 3b：GLM、Kimi、OpenCode Go |
| S4 | Task 4 + Task 5 |

Task 6 客户端在 Task 1 后另开一个会话。4 路并行的前提是 Task 2 先定义好 `QuotaAdapterKind` 和注册表；S2、S3 只写自己的文件。

### 不建议

- 同一目录开两个会话改 `service/quota/mod.rs`
- 两个会话分别改 `prelay-server/crates/protocol` 和 `prelay-client/crates/protocol`
- 记录日志改动未提交时直接开额度并行会话
- 在 Task 1 未完成前开始 Task 4 或 Task 6

## 验收标准

- 7 家供应商中 6 家有自动额度，GoToken 显示未接入
- 同一张卡可以同时展示窗口和余额
- 列表首屏不因上游慢而串行等待
- 上游 5xx 时保留上次读数并标 stale
- 共享额度并发请求不会一起穿过上限
- 千问不再出现在新建供应商列表
- 协议、服务端、客户端测试全部通过

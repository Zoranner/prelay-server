# 成本移除与全用户排行榜实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task with one commit per completed step.

**Goal:** 从 Prelay 完全移除成本统计，并为普通设备身份提供团队内所有用户的活动聚合排行榜。

**Architecture:** 成本能力从协议 DTO、活动实体、价格配置和统计聚合中整体删除；对既有数据库执行幂等的旧列清理。排行榜复用 `identity_activities` 与 `identities`，新增普通设备凭据可访问的全局聚合接口，仅返回显示名和非敏感统计指标。

**Tech Stack:** Rust, Axum, SeaORM, SQLite, PostgreSQL, `prelay-protocol`, Tauri client, Bun tests.

**Spec:** `docs/superpowers/specs/2026-08-31-activity-memory-design.md`

## Global Constraints

- 领域术语统一使用“活动”。
- 排行榜允许普通设备身份查看所有用户，但只返回聚合数据和显示名。
- 不返回 `machine_id`、`account_sid`、凭据、活动正文、供应商配置或错误详情。
- 涉及 DTO 或管理 API 时，先修改 `prelay-protocol`，再更新服务端和客户端 submodule。
- 不修改公开 `/v1` 协议。
- 不启动服务、浏览器、Tauri 或 Compose。
- 每个完成步骤单独提交，不跨任务攒提交。
- 不推送、不打 tag、不发布。

### Task 1: 移除成本协议字段

**Files:**
- Modify: `prelay-protocol/src/stats.rs`
- Test: `prelay-protocol/tests/management_dto.rs`

- [ ] 删除 `ModelStatsSummary.estimated_cost` 与 `ProviderStatsSummary.estimated_cost`。
- [ ] 更新 DTO round-trip 测试，确认 JSON 不再包含成本字段。
- [ ] 运行 `cargo fmt --all` 与协议测试、Clippy。
- [ ] 提交 `移除成本统计协议字段`。

### Task 2: 移除服务端成本计算与数据库字段

**Files:**
- Modify: `prelay-server/src/stats.rs`
- Modify: `prelay-server/src/storage/activities.rs`
- Modify: `prelay-server/src/storage/stats.rs`
- Modify: `prelay-server/src/entity/identity/activities.rs`
- Modify: `prelay-server/src/schema/tables/activities.rs`
- Modify: `prelay-server/src/schema/mod.rs`
- Modify: `prelay-server/tests/schema/contract.rs`
- Modify: `prelay-server/src/storage/activities.rs` tests
- Delete: `prelay-server/config/model_prices.example.json`
- Modify: `prelay-server/deploy/.env.example`
- Modify: `prelay-server/README.md`

- [ ] 删除价格类型、价格文件读取、估算函数及写入/更新逻辑。
- [ ] 删除活动实体和新建 schema 中的成本列。
- [ ] 增加 SQLite/PostgreSQL 幂等清理既有 `estimated_cost` 与 `currency` 列的初始化步骤，并覆盖 schema 测试。
- [ ] 删除成本测试、价格示例和部署说明。
- [ ] 运行服务端 Rust 全量门禁和 schema 测试。
- [ ] 提交 `移除服务端成本统计`。

### Task 3: 清理客户端成本消费

**Files:**
- Modify: `prelay-client/app/stores/relay.ts`
- Modify: any client activity/statistics view or type that still consumes `estimated_cost`
- Update: `prelay-client/crates/protocol` submodule pointer

- [ ] 删除客户端成本字段和显示逻辑。
- [ ] 更新协议 submodule 到 Task 1 提交。
- [ ] 运行 Bun 测试、typecheck，以及客户端 Rust fmt/Clippy/test。
- [ ] 提交 `清理客户端成本统计`。

### Task 4: 定义全用户排行榜协议

**Files:**
- Modify: `prelay-protocol/src/stats.rs`
- Test: `prelay-protocol/tests/management_dto.rs`

**Interface:**

```rust
pub struct LeaderboardQuery {
    pub range: StatsRange,
    pub metric: LeaderboardMetric,
    pub limit: usize,
}

pub enum LeaderboardMetric {
    Activities,
    TotalTokens,
    SuccessfulActivities,
    SuccessRate,
}

pub struct UserLeaderboardEntry {
    pub rank: i64,
    pub identity_id: String,
    pub display_name: String,
    pub activity_count: i64,
    pub total_tokens: i64,
    pub successful_activities: i64,
    pub success_rate: f64,
}
```

- [ ] 采用稳定字段和 `snake_case` 序列化。
- [ ] 增加 round-trip 与默认 limit/range 测试。
- [ ] 提交 `定义用户活动排行榜协议`。

### Task 5: 实现服务端全用户排行榜

**Files:**
- Modify: `prelay-server/src/storage/stats.rs`
- Modify: `prelay-server/src/routes/api/stats.rs`
- Test: `prelay-server/tests/management/stats.rs`

**Interface:**

```text
GET /api/stats/leaderboard?range=today&metric=activities&limit=50
```

- [ ] 按 `identity_activities.identity_id` 聚合，并关联 `identities.display_name`。
- [ ] 支持活动次数、总 Token、成功次数、成功率四种指标。
- [ ] 仅使用当前普通设备凭据进行认证，不按当前 `identity_id` 过滤。
- [ ] 将 limit 限制在 1..=100，按指标降序、identity_id 升序稳定排序并生成 rank。
- [ ] 对空显示名使用稳定的匿名展示名，不泄露设备定位字段。
- [ ] 增加跨两个身份的集成测试，验证普通身份可看到双方且不返回敏感字段。
- [ ] 运行服务端全量 Rust 门禁。
- [ ] 提交 `增加全用户活动排行榜`。

### Task 6: 接入客户端排行榜

**Files:**
- Modify: `prelay-client/app/stores/relay.ts`
- Create or modify: client statistics/leaderboard view following existing activity page structure
- Update: `prelay-client/crates/protocol` submodule pointer
- Test: focused Bun statistics tests

- [ ] 调用 `/api/stats/leaderboard` 的 Tauri 原生命令链，不让 Nuxt 页面直接访问管理 API。
- [ ] 默认展示活动次数，可切换时间范围和排行指标。
- [ ] 展示 rank、display_name、活动次数、Token、成功率，不展示成本。
- [ ] 运行 focused Bun test、typecheck 和客户端 Rust 门禁。
- [ ] 提交 `展示全用户活动排行榜`。

### Task 7: 收口验证

- [ ] 分别检查三个仓库工作树和 submodule 指针。
- [ ] 运行服务端 `cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features`、`git diff --check`。
- [ ] 运行协议和客户端对应验证。
- [ ] 确认没有启动服务、推送、tag 或 Release。
- [ ] 如需版本号调整，单独建立版本提交，不混入功能提交。

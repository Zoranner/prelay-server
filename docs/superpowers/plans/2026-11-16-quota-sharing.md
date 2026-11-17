# 供应商额度与共享实施计划

> 本计划是额度 v1 的历史记录，新的实现边界和并行方案见 `2026-11-17-quota-v2-adapters-and-sharing.md`。

**Goal:** 在供应商列表额度列展示套餐、余额和共享额度，并把共享改成按额度共享。

**Architecture:** 协议定义额度 DTO；服务端用供应商适配器获取套餐/余额，用活动记录计算共享限额；客户端用紧凑额度列和独立共享抽屉消费。

**Tech Stack:** Rust、Axum、SeaORM、PostgreSQL、Vue 3、Nuxt、Tauri、Bun。

**Spec:** `prelay-server/docs/superpowers/specs/2026-11-16-quota-sharing-design.md`

## Global Constraints

- 不新增独立额度页面。
- 不在客户端手填套餐或余额数值。
- 共享设置与额度查看分开。
- 额度列替换可见范围列。
- 创建者查看被共享人用量暂不实现。
- 提交、推送、发布分别授权。

---

### Task 1: 协议额度 DTO

**Files:**
- Modify: `prelay-protocol/src/providers.rs`
- Test: `prelay-server/crates/protocol/tests/management_dto.rs`

**Produces:** `QuotaWindowKind`、`ShareQuotaWindow`、`ProviderShareInput`、`ProviderShareResponse`、`ProviderQuotaMode`、`ProviderQuota`、`QuotaWindowResponse`、`ProviderQuotaResponse`。

- [ ] 增加 DTO 和 serde 标签。
- [ ] 把共享响应/请求改为 `shares` 全量数组。
- [ ] 更新协议测试，覆盖序列化、反序列化和错误码不变。
- [ ] 运行 `cargo test -p prelay-protocol`。

### Task 2: 共享额度存储

**Files:**
- Modify: `prelay-server/src/store/schema/tables/provider_shares.rs`
- Modify: `prelay-server/src/store/entity/identity/provider_shares.rs`
- Modify: `prelay-server/src/store/schema/mod.rs`
- Modify: `prelay-server/src/store/schema/validation.rs`
- Modify: `prelay-server/src/store/storage/provider_visibility.rs`
- Test: `prelay-server/tests/management/provider_sharing.rs`

**Consumes:** Task 1 DTO。

**Produces:** `quota_json` 列；共享读取/保存支持直接共享和限额共享。

- [ ] 表增加 `quota_json TEXT`，迁移用 `ADD COLUMN IF NOT EXISTS`。
- [ ] 实体增加 `quota_json: Option<String>`。
- [ ] 保存共享时校验身份、去重、禁止拥有者、校验窗口规则。
- [ ] 读取共享时解析 `quota_json`，返回身份显示名。
- [ ] 测试全量替换、直接共享、限额共享、非法规则。

### Task 3: 额度快照与适配器

**Files:**
- Create: `prelay-server/src/service/quota/mod.rs`
- Create: `prelay-server/src/service/quota/deepseek.rs`
- Create: `prelay-server/src/service/quota/minimax.rs`
- Create: `prelay-server/src/store/storage/provider_quota.rs`
- Modify: `prelay-server/src/lib.rs`
- Modify: `prelay-server/src/service/mod.rs`
- Modify: `prelay-server/src/store/storage/mod.rs`
- Modify: `prelay-server/src/bootstrap/*`
- Test: `prelay-server/src/service/quota/tests.rs`

**Consumes:** Task 1 DTO、Task 2 共享规则。

**Produces:** `QuotaService::snapshot`、`Storage::list_provider_quotas`。

- [ ] DeepSeek 余额解析与错误映射。
- [ ] MiniMax `general` 桶 5 小时/周窗口解析。
- [ ] 60 秒缓存，按供应商和密钥摘要隔离。
- [ ] 共享限额窗口计算：日/周/月边界、累计、上一周期。
- [ ] 列表额度按拥有者/直接共享/限额共享/未接入分类。
- [ ] 请求解析时共享限额耗尽则跳过该 Provider。
- [ ] 纯函数测试覆盖解析和周期边界。

### Task 4: 额度接口与列表接线

**Files:**
- Modify: `prelay-server/src/inbound/api/providers.rs`
- Modify: `prelay-server/src/inbound/api/mod.rs`（如路由模块需要）
- Test: `prelay-server/tests/management/provider_usage.rs` 或新增额度测试

**Consumes:** Task 3。

- [ ] 增加 `GET /api/providers/quotas`。
- [ ] 共享接口改用新 DTO。
- [ ] 集成测试覆盖三种额度形态和不可见供应商。
- [ ] 运行 `cargo test --test management`。

### Task 5: 客户端额度列

**Files:**
- Modify: `prelay-client/app/types/relay.ts`
- Modify: `prelay-client/app/composables/command/relay.ts`
- Modify: `prelay-client/src-tauri/src/commands/providers.rs`
- Modify: `prelay-client/src-tauri/src/app/mod.rs`
- Modify: `prelay-client/app/pages/providers.vue`
- Modify: `prelay-client/app/components/providers/List.vue`
- Create: `prelay-client/app/components/providers/quota/Cell.vue`
- Create: `prelay-client/app/components/providers/quota/Rings.vue`
- Delete: `prelay-client/app/components/providers/sharing/VisibilityScope.vue`
- Test: `prelay-client/tests/provider-flow.test.ts`

**Consumes:** Task 4。

- [ ] 增加 `providers_quotas` 命令和类型。
- [ ] 供应商列表用额度列替换可见范围列。
- [ ] 三环、余额、共享进度、未接入四种紧凑形态。
- [ ] 加载态、错误态、空态。
- [ ] 恢复行内共享按钮。
- [ ] 更新源文本测试。

### Task 6: 共享抽屉额度规则

**Files:**
- Modify: `prelay-client/app/components/providers/sharing/Drawer.vue`
- Modify: `prelay-client/app/composables/providerSharing.ts`
- Modify: `prelay-client/tests/provider-flow.test.ts`

**Consumes:** Task 4、Task 5。

- [ ] 抽屉只保留共享配置，移除统计区和用量命令调用。
- [ ] 每个被共享人支持不限额/限额。
- [ ] 限额支持日/周/月窗口、累计、清零时间。
- [ ] 保存后刷新供应商列表和额度。
- [ ] 更新测试并运行 `bun test`、`bun run typecheck`、`bun run lint`。

### Task 7: 收口验证

- [ ] 协议：`cargo test`。
- [ ] 服务端：`cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-features`。
- [ ] 客户端：`bun run format:check`、`bun run lint`、`bun run typecheck`、`bun test`。
- [ ] 记录未覆盖边界：真实上游额度接口未用生产密钥验证；并发请求的原子预留未实现。

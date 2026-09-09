# Provider Sharing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在同一个 `prelay-server` 实例内实现 Provider 分享、跨身份只读引用和对所有可见用户开放的使用统计。

**Architecture:** 保留 `identity_provider_configs` 作为 Provider 唯一实体，在 Provider 上保存 `private`、`selected`、`all` 可见范围，并用 `identity_provider_shares` 保存指定身份授权。所有 Provider 列表、Endpoint 保存、Endpoint 读取和 `/v1` 路由解析统一调用服务端可见性判断；不复制 Provider 或 API Key。

**Tech Stack:** Rust、Axum、SeaORM/SeaQuery、SQLite/PostgreSQL、Serde、Tauri 2、Nuxt 4、Vue、Bun、`@stellar/ui`

**Spec:** `prelay-server/docs/superpowers/specs/2026-09-08-provider-sharing-design.md`

## Global Constraints

- 仅允许同一个 `prelay-server` 实例内已注册身份之间分享。
- 分享范围固定为 `private`、`selected`、`all`。
- 非创建人只能查看、引用和查看统计，不能编辑、删除、Ping、协议测试或获得 API Key。
- Provider API Key 只由服务端使用 `ENCRYPTION_KEY` 加密保存和解密调用。
- 撤销分享或删除 Provider 后，后续 Endpoint 保存和协议路由立即失效。
- 所有能够看到 Provider 的用户都可以查看完整使用统计和使用者明细。
- 本期不实现配额、计费、限流、分享链接、跨实例分享、组织和角色。
- 协议 DTO 必须先在 `prelay-protocol` 修改，再更新两个父仓的 submodule 指针。
- 不修改或覆盖三个仓库当前与本任务无关的未提交改动。
- Rust 修改后执行 `cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features` 和 `git diff --check`；Node/Nuxt 修改只使用 Bun。

## File Map

### `prelay-protocol`

- Modify `src/providers.rs`: 增加可见性、Provider 列表项、创建人、分享设置和使用统计 DTO。
- Modify `src/identity.rs`: 增加可授权身份的非敏感展示 DTO。
- Modify `src/error.rs`: 增加分享无效、无权管理和共享 Provider 不可用的稳定错误码。
- Modify `tests/management_dto.rs`: 覆盖新增 DTO、枚举和敏感字段边界。

### `prelay-server`

- Modify `src/schema/tables/providers.rs`: 为 Provider 保存当前分享范围。
- Create `src/schema/tables/provider_shares.rs`: 定义指定身份授权关系表。
- Modify `src/schema/tables/mod.rs`, `src/schema/mod.rs`, `src/schema/indexes.rs`: 注册新表、初始化和唯一索引。
- Modify `src/entity/identity/provider_configs.rs`: 映射 `visibility` 和创建人信息所需字段。
- Create `src/entity/identity/provider_shares.rs`: 映射授权关系。
- Modify `src/entity/identity/mod.rs`: 导出新实体。
- Create `src/storage/provider_visibility.rs`: 提供唯一的 Provider 可见性和管理权限判断。
- Create `src/storage/provider_usage.rs`: 聚合可见 Provider 的请求和 Token 统计。
- Modify `src/storage/providers.rs`, `src/storage/endpoints.rs`, `src/storage/access.rs`, `src/storage/identities.rs`, `src/storage/mod.rs`: 接入共享 Provider、授权替换、删除清理、身份展示和协议访问。
- Modify `src/routes/api/providers.rs`: 返回可见 Provider、管理分享、读取统计。
- Modify `src/routes/api/identities.rs`, `src/routes/api/mod.rs`: 返回可授权身份的非敏感列表。
- Modify `tests/management/providers.rs`, `tests/management/endpoint_access.rs`, `tests/management/endpoints.rs`, `tests/v1/identity_scope.rs`, `tests/schema/initialization.rs`, `tests/schema/contract.rs`: 覆盖接口、身份隔离、迁移和删除边界。
- Create `tests/management/provider_sharing.rs`: 集中覆盖分享状态、授权关系和统计权限。

### `prelay-client`

- Modify `src-tauri/src/commands/providers.rs`: 增加分享设置、共享 Provider 使用统计和授权身份命令。
- Modify `src-tauri/src/commands/identity.rs`, `src-tauri/src/commands/mod.rs`: 提供当前身份可用的身份目录命令并注册命令。
- Modify `app/composables/useRelayCommand.ts`: 注册新增 Tauri command 名称。
- Modify `app/stores/relay.ts`: 增加共享 Provider、创建人、可见性和统计类型。
- Modify `app/components/providers/ProviderList.vue`: 展示创建人、分享状态、统计摘要和只读操作状态。
- Create `app/components/providers/ProviderSharingDrawer.vue`: 管理分享设置和完整使用统计。
- Modify `app/pages/providers.vue`: 加载可见 Provider、打开分享抽屉、提交授权、刷新状态。
- Modify `app/components/endpoints/EndpointForm.vue`, `app/utils/endpointModels.ts`, `app/pages/endpoints.vue`: 将共享 Provider 作为只读候选并保存引用。
- Modify `tests/provider-flow.test.ts`, `tests/provider-operation-result.test.ts`, `tests/endpoint-flow.test.ts`, `tests/endpoint-models.test.ts`: 覆盖共享 Provider 展示、只读行为、Endpoint 引用和刷新失效。

---

### Task 1: 扩展共享协议契约

**Files:**
- Modify: `prelay-protocol/src/providers.rs`
- Modify: `prelay-protocol/src/identity.rs`
- Modify: `prelay-protocol/src/error.rs`
- Modify: `prelay-protocol/src/lib.rs`
- Test: `prelay-protocol/tests/management_dto.rs`

**Interfaces:**
- Produces `ProviderVisibility::{Private, Selected, All}` serialized as `private`, `selected`, `all`.
- Produces `ProviderOwner { identity_id: String, display_name: String }`.
- Produces `ProviderListItemResponse` without `api_key`.
- Produces `ProviderSharingResponse { visibility, selected_identity_ids, can_manage }`.
- Produces `UpdateProviderSharingRequest { visibility, identity_ids }`.
- Produces `ProviderUsageResponse` with total counters and `users: Vec<ProviderUsageUser>`.
- Produces `IdentityDirectoryEntry { identity_id, display_name }`.

- [ ] **Step 1: Write failing DTO tests** for enum serialization, selected identity payload, provider list response without an API Key field, and usage response counters.
- [ ] **Step 2: Run the focused protocol tests**.

Run:

```text
cargo test --manifest-path prelay-protocol/Cargo.toml --test management_dto
```

Expected: FAIL because the new types and fields do not exist.

- [ ] **Step 3: Add the DTOs and error codes** without importing HTTP, storage, database, or client-specific types into the protocol crate.
- [ ] **Step 4: Re-run protocol formatting, Clippy, and tests**.

Run:

```text
cargo fmt --manifest-path prelay-protocol/Cargo.toml --all
cargo clippy --manifest-path prelay-protocol/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path prelay-protocol/Cargo.toml --all-targets --all-features
```

- [ ] **Step 5: Inspect the protocol diff** and confirm no credential field was added to shared list, sharing, identity directory, or usage DTOs.

### Task 2: Add the server schema and SeaORM entities

**Files:**
- Modify: `prelay-server/src/schema/tables/providers.rs`
- Create: `prelay-server/src/schema/tables/provider_shares.rs`
- Modify: `prelay-server/src/schema/tables/mod.rs`
- Modify: `prelay-server/src/schema/mod.rs`
- Modify: `prelay-server/src/schema/indexes.rs`
- Modify: `prelay-server/src/entity/identity/provider_configs.rs`
- Create: `prelay-server/src/entity/identity/provider_shares.rs`
- Modify: `prelay-server/src/entity/identity/mod.rs`
- Test: `prelay-server/tests/schema/initialization.rs`
- Test: `prelay-server/tests/schema/contract.rs`

**Interfaces:**
- Produces `identity_provider_configs.visibility`.
- Produces `identity_provider_shares(provider_id, grantee_identity_id, created_at)`.
- Produces unique index `uq_identity_provider_shares_provider_grantee`.
- Existing empty-database initialization must create both structures.

- [ ] **Step 1: Add schema contract tests** asserting the new column, table, unique index, foreign keys, and absence of credentials in the sharing table.
- [ ] **Step 2: Run the schema tests** and verify they fail against the current schema.
- [ ] **Step 3: Implement the SeaQuery table definitions** with foreign keys to Provider and Identity and the unique provider/grantee constraint.
- [ ] **Step 4: Add the SeaORM models and relations** and include the new table in schema initialization. Preserve the existing incomplete-schema rejection behavior rather than silently accepting a partial deployment.
- [ ] **Step 5: Run focused schema tests and formatting**.

Run:

```text
cargo test --test schema::initialization
cargo test --test schema::contract
cargo fmt --all
```

Expected: PASS, with the existing unrelated working-tree changes preserved.

### Task 3: Implement one server-side visibility service

**Files:**
- Create: `prelay-server/src/storage/provider_visibility.rs`
- Modify: `prelay-server/src/storage/providers.rs`
- Modify: `prelay-server/src/storage/endpoints.rs`
- Modify: `prelay-server/src/storage/mod.rs`
- Test: `prelay-server/tests/management/provider_sharing.rs`

**Interfaces:**
- `Storage::list_visible_providers(identity_id: &str) -> Result<Vec<ProviderListItemResponse>, StorageError>`
- `Storage::get_visible_provider(identity_id: &str, provider_id: &str) -> Result<VisibleProvider, StorageError>`
- `Storage::can_use_provider(identity_id: &str, provider_id: &str) -> Result<bool, StorageError>`
- `Storage::get_provider_sharing(identity_id: &str, provider_id: &str) -> Result<ProviderSharingResponse, StorageError>`
- `Storage::update_provider_sharing(identity_id: &str, provider_id: &str, input: UpdateProviderSharingRequest) -> Result<ProviderSharingResponse, StorageError>`

- [ ] **Step 1: Write failing storage/API tests** for `private`, `selected`, and `all`, including an identity that cannot distinguish “not visible” from “not found”.
- [ ] **Step 2: Implement the visibility query** as the only source for list and use authorization:
  - Always include Providers owned by the current identity.
  - Include `all` Providers for every registered identity.
  - Include `selected` Providers only when a matching share row exists.
  - Exclude `private` Providers owned by another identity.
- [ ] **Step 3: Implement full-state share replacement in one transaction**:
  - Verify the current identity owns the Provider.
  - Reject non-empty identity IDs for `private` and `all`.
  - Verify every selected identity exists.
  - Reject the owner as a selected grantee.
  - Replace all selected rows rather than applying partial row patches.
- [ ] **Step 4: Update Provider list/detail responses** so shared entries expose owner display information and no API Key. Keep owner-only edit/detail behavior explicit.
- [ ] **Step 5: Run `provider_sharing` tests** and add regression coverage for owner management and non-owner read-only behavior.

### Task 4: Enforce sharing in Endpoint and protocol paths

**Files:**
- Modify: `prelay-server/src/storage/endpoint_validation.rs`
- Modify: `prelay-server/src/storage/endpoints.rs`
- Modify: `prelay-server/src/storage/access.rs`
- Modify: `prelay-server/src/storage/providers.rs`
- Modify: `prelay-server/src/routes/v1/endpoint_resolver.rs`
- Test: `prelay-server/tests/management/endpoints.rs`
- Test: `prelay-server/tests/management/endpoint_access.rs`
- Test: `prelay-server/tests/v1/identity_scope.rs`
- Test: `prelay-server/tests/management/provider_sharing.rs`

**Interfaces:**
- Endpoint model validation must call `Storage::can_use_provider`.
- Protocol model loading must resolve Provider configuration only when the Endpoint owner can currently use it.
- Provider deletion must remove or invalidate every Endpoint model/route reference to that Provider across identities in the same transaction.

- [ ] **Step 1: Add failing tests** for creating/updating an Endpoint with a visible shared Provider, rejecting an invisible Provider, and rejecting a previously valid reference after revocation.
- [ ] **Step 2: Change Endpoint model validation** to use the shared visibility service rather than filtering only by `provider_configs.identity_id`.
- [ ] **Step 3: Change protocol access resolution** to skip revoked or deleted shared Providers and return the existing no-candidate behavior when no authorized candidate remains.
- [ ] **Step 4: Enforce owner-only Provider mutations** for edit, delete, Ping, and protocol tests. A non-owner must receive the stable permission error without an API Key lookup.
- [ ] **Step 5: Make Provider deletion clean cross-identity Endpoint model and route references** before deleting the Provider and its share rows.
- [ ] **Step 6: Run focused management and `/v1` identity-scope tests**.

Run:

```text
cargo test --test management::endpoints
cargo test --test management::endpoint_access
cargo test --test v1::identity_scope
cargo test --test management::provider_sharing
```

### Task 5: Add shared Provider usage statistics

**Files:**
- Create: `prelay-server/src/storage/provider_usage.rs`
- Modify: `prelay-server/src/storage/mod.rs`
- Modify: `prelay-server/src/routes/api/providers.rs`
- Modify: `prelay-server/src/routes/api/stats.rs` only if existing aggregation helpers are the correct shared location
- Test: `prelay-server/tests/management/provider_sharing.rs`
- Test: `prelay-server/tests/management/stats.rs`

**Interfaces:**
- `Storage::get_visible_provider_usage(identity_id: &str, provider_id: &str, range: StatsRange) -> Result<ProviderUsageResponse, StorageError>`
- `GET /api/providers/:provider_id/usage`

- [ ] **Step 1: Add failing tests** with activities from multiple identities using the same Provider, including successful and failed requests and null Token values.
- [ ] **Step 2: Implement aggregation from existing activity records** grouped by Provider and actual `identity_id`; calculate request count, input Token, output Token, total Token, and latest activity timestamp.
- [ ] **Step 3: Gate the usage query with the same Provider visibility service** so every visible user receives the same complete aggregate and user breakdown.
- [ ] **Step 4: Define response behavior for a visible Provider with no usage** as zero counters and an empty user list, not a missing-resource error.
- [ ] **Step 5: Verify the response contains only display identities and usage counters**, never API Key, device credential, Endpoint Token, upstream request body, or response content.
- [ ] **Step 6: Run focused statistics tests** and compare the aggregation field meanings with the existing dashboard statistics before changing any shared statistic semantics.

### Task 6: Expose the server contract through Tauri commands

**Files:**
- Modify: `prelay-client/src-tauri/src/commands/providers.rs`
- Modify: `prelay-client/src-tauri/src/commands/identity.rs`
- Modify: `prelay-client/src-tauri/src/commands/mod.rs`
- Modify: `prelay-client/app/composables/useRelayCommand.ts`
- Modify: `prelay-client/app/stores/relay.ts`
- Test: `prelay-client/tests/provider-flow.test.ts`
- Test: `prelay-client/tests/provider-operation-result.test.ts`

**Interfaces:**
- Tauri commands:
  - `providers_sharing_get(provider_id: String)`
  - `providers_sharing_save(provider_id: String, input: ProviderSharingInput)`
  - `providers_usage_get(provider_id: String, range: StatsRange)`
  - `identity_directory_list()`
- All commands use `authenticated_api`; Nuxt code does not call the management API directly.

- [ ] **Step 1: Add failing command-flow tests** asserting the exact management API paths and that shared Provider responses are treated as read-only.
- [ ] **Step 2: Add protocol-backed Rust command inputs and outputs**; do not define duplicate DTOs for protocol types.
- [ ] **Step 3: Add the identity directory command** returning only `identity_id` and `display_name`.
- [ ] **Step 4: Register the commands and update `RelayCommand`**.
- [ ] **Step 5: Run the focused Bun tests and Tauri Rust tests**.

Run:

```text
bun test tests/provider-flow.test.ts tests/provider-operation-result.test.ts
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
```

### Task 7: Implement Provider sharing and usage UI

**Files:**
- Modify: `prelay-client/app/stores/relay.ts`
- Modify: `prelay-client/app/components/providers/ProviderList.vue`
- Create: `prelay-client/app/components/providers/ProviderSharingDrawer.vue`
- Modify: `prelay-client/app/pages/providers.vue`
- Modify: `prelay-client/app/composables/useProviderForm.ts` only if shared read-only form state requires a shared helper
- Test: `prelay-client/tests/provider-flow.test.ts`

**Interfaces:**
- `ProviderList.vue` receives Provider list items with `owner_display_name`, `visibility`, and read-only state.
- `ProviderSharingDrawer.vue` emits `save-sharing`, `close`, and receives sharing DTO, identity directory, usage DTO, and pending state.
- `providers.vue` remains the single page-level state owner for Provider list, sharing drawer, and notifications.

- [ ] **Step 1: Add failing component-flow assertions** for owner and non-owner rows, creator column, visibility label, hidden mutation actions, and complete usage display.
- [ ] **Step 2: Add the creator and sharing columns** to the existing Provider list without introducing a second Provider page.
- [ ] **Step 3: Add the sharing drawer** with `private`/`selected`/`all` controls, selected identity management, full usage counters, trend, and per-user details.
- [ ] **Step 4: Make the drawer load usage for every visible Provider** and show zero-state statistics when no activity exists.
- [ ] **Step 5: Disable edit, delete, Ping, and protocol test controls for non-owner Providers** while keeping Endpoint reference available.
- [ ] **Step 6: Refresh Provider and Endpoint state after sharing changes, revocation, and deletion so stale visible entries cannot remain actionable.
- [ ] **Step 7: Run focused Bun tests and `bun run typecheck`**.

### Task 8: Allow shared Providers in Endpoint configuration

**Files:**
- Modify: `prelay-client/app/utils/endpointModels.ts`
- Modify: `prelay-client/app/components/endpoints/EndpointForm.vue`
- Modify: `prelay-client/app/components/endpoints/EndpointList.vue` only if source labels are displayed there
- Modify: `prelay-client/app/pages/endpoints.vue`
- Test: `prelay-client/tests/endpoint-flow.test.ts`
- Test: `prelay-client/tests/endpoint-models.test.ts`

**Interfaces:**
- Endpoint Provider options include the original `provider_id`, Provider display name, creator display name, and `read_only` state.
- Save payload remains `EndpointModelInput { provider_id, upstream_model }`; no copied Provider fields or credentials are added.

- [ ] **Step 1: Add failing tests** for selecting a shared Provider, retaining its original ID in the save payload, and removing a revoked Provider after reload.
- [ ] **Step 2: Extend endpoint option grouping** to distinguish own and shared Providers using metadata, while preserving existing model compatibility filtering.
- [ ] **Step 3: Keep shared Provider fields read-only** in the Endpoint form; only the model mapping and Endpoint itself are editable.
- [ ] **Step 4: Handle server rejection after revocation** by refreshing the Provider/Endpoint data and showing the returned stable error.
- [ ] **Step 5: Run focused endpoint tests and typecheck**.

### Task 9: Cross-repository verification and handoff

**Files:**
- Verify all files listed above
- Verify: `prelay-server/docs/superpowers/specs/2026-09-08-provider-sharing-design.md`
- Update: submodule pointers in `prelay-server/crates/protocol` and `prelay-client/crates/protocol` only after the protocol repository change is committed and authorized

- [ ] **Step 1: Run protocol verification** from `prelay-protocol`.
- [ ] **Step 2: Run server formatting, strict Clippy, all tests, and diff check** without using an alternate Cargo target directory.
- [ ] **Step 3: Run client Bun tests, typecheck, and generate; run Tauri formatting, strict Clippy, and tests if Rust commands changed.**
- [ ] **Step 4: Run cross-boundary tests** covering:
  - private, selected, and all visibility;
  - owner-only mutation;
  - shared Provider Endpoint reference;
  - revoke/delete immediate route invalidation;
  - complete statistics visible to every visible identity;
  - no credential exposure in any response.
- [ ] **Step 5: Inspect each repository status separately** and classify unrelated existing changes as excluded from the delivery.
- [ ] **Step 6: Report any blocked PostgreSQL, running-process, or client runtime verification separately from local test results.**
- [ ] **Step 7: Do not commit, push, tag, publish, or change deployment state without separate explicit authorization.**

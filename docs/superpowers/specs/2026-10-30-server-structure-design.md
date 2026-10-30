# 服务端目录结构设计

本设计取代 `docs/superpowers/specs/2026-08-29-server-structure-design.md` 中的目录约定。那份文档描述的 `messages` 协议和 SQLite 支持已经移除，但它的两条原则继续有效：目录表达职责，单个 Rust 文件不超过 450 行。本文只重新定义顶层归属规则和目标目录。

## 问题

`src/` 现在有 203 个 Rust 文件、28,870 行，单文件最大 407 行，低于 450 行门禁。问题不在文件规模，而在顶层没有统一维度。

- 根目录成了默认落点：12 个 `.rs` 文件，其中 10 个是实现文件（约 1,856 行），另有 14 个目录。`image_budget`、`protocol_body_limit`、`failover`、`client_update` 都是直接加到根目录的。
- 顶层同时存在两套切分方式：技术层（`routes`、`storage`、`entity`、`schema`、`bridge`、`providers`、`observability`）和领域切片（`activity`、`memory`、`identity`），同类职责处在不同深度。`extensions/` 是 1,004 行的目录，`client_update.rs` 是 361 行的根文件（含测试 495 行），两者都是远端制品缓存。
- 持久化被拆成四个顶层：`database.rs` 91 行、`schema/` 1,954 行、`entity/` 452 行、`storage/` 5,133 行。一个功能改动经常同时涉及其中三到四个。
- 命名互相冲突：`provider_catalog`（配置目录）与 `providers`（上游协议编解码）都叫 provider；`stats.rs`（领域口径）与 `storage/stats/`（查询实现）同名不同层；`identity/credential.rs`（30 行）与 `storage/identities.rs`（330 行）分居两处。
- 职责错位：`main.rs` 359 行里除了装配还有清理协程、刷新协程和 rollout 目录诊断；`routes/api/provider_protocol_test.rs` 是被 `routes/api/providers.rs` 调用的生产代码，却按测试文件命名。
- `activity::ActivityStore` 依赖 `storage::Storage`，而 `storage/rollouts.rs`、`storage/rollout_recovery.rs` 又依赖 `activity::rollout::{RolloutStatus, RolloutSummary}`，事务协调和持久化记录互相依赖。

只按现有目录深挖，下一轮仍会在根目录追加文件、在顶层追加目录。本设计把顶层固定为一个维度：依赖方向。

## 设计规则

顶层只放架构层，每个层只有一个归属判据；业务能力、协议和存储介质都下沉到所属层内部。新增顶层目录视为结构变更，必须同时修改本文和结构门禁。

依赖只允许自上而下：`domain` 在最底层，`bootstrap` 在最上层。同一层内部按能力再分目录，但不再改变层数。

## 目标结构

```text
src/
  main.rs
  lib.rs
  error.rs

  bootstrap/
    mod.rs
    environment.rs
    serve.rs
    router.rs
    tasks.rs
    diagnostics.rs
    test_support.rs

  inbound/
    mod.rs
    limits.rs
    api/
    v1/
      responses/
        image_budget.rs

  service/
    mod.rs
    activity.rs
    recording/
    upstream_errors.rs
    failover.rs
    cleanup.rs
    provider_check.rs

  codec/
    mod.rs
    internal.rs
    usage.rs
    bridge/
    providers/

  outbound/
    mod.rs
    client.rs
    integrations/
      extensions/
      client_update/

  store/
    mod.rs
    connect.rs
    schema/
    entity/
    storage/
    rollout/

  domain/
    mod.rs
    registry.rs
    stats.rs
    catalog/
    activity/
    memory/
    identity/
```

顶层 7 个目录，根目录 3 个文件。

## 层职责

| 层 | 判据 | 典型内容 |
| --- | --- | --- |
| `domain` | 只有类型、校验和纯计算；不访问网络和数据库 | 供应商与接入点登记记录、统计口径、供应商目录模型、活动事件与脱敏、记忆规范化、设备凭据生成 |
| `codec` | 输入输出都是内存值的协议转换；不构造 HTTP 请求 | 内部规范模型、上下游协议编解码、用量解析、供应商能力与地址解析 |
| `store` | 只操作单一介质，不含跨介质流程 | PostgreSQL 连接、DDL、SeaORM 实体、查询、rollout 文件读写 |
| `outbound` | 所有真实网络调用与远端制品缓存 | 上游 HTTP 客户端与超时策略、扩展制品、客户端安装包缓存 |
| `service` | 同时使用多个介质或多个协议的用例编排 | 活动记录协调、流式统计记录、供应商冷却、过期身份清理、供应商连通性检查 |
| `inbound` | HTTP 解析、状态码、路由注册、请求体上限 | 管理 API、协议 API、请求体上限、图片预算 |
| `bootstrap` | 进程生命周期与装配 | 环境读取、子系统装配、路由挂载、后台任务、启动诊断、测试夹具 |

判断新文件归属时按此表自上而下匹配第一条成立的判据，不按“放在哪里方便引用”决定。

## 迁移映射

| 现有 | 目标 | 说明 |
| --- | --- | --- |
| `main.rs` | `main.rs` + `bootstrap/{environment,serve,tasks,diagnostics}.rs` | 入口只保留 `#[tokio::main]` 与一次 `bootstrap::serve()` 调用 |
| `app.rs` | `bootstrap/router.rs` | `/api`、`/v1` 挂载 |
| `lib.rs` 内 `test_support` | `bootstrap/test_support.rs` | 编译条件本次不变 |
| `error.rs` 中 `impl From<StorageError> for AppError` | `store/storage/error.rs` | 错误适配放在错误源一侧，使 `error.rs` 成为叶子 |
| `routes/api/` | `inbound/api/` | 管理 API |
| `routes/v1/` | `inbound/v1/` | 协议 API |
| `protocol_body_limit.rs` | `inbound/limits.rs` | 协议入口请求体上限 |
| `image_budget.rs`、`image_budget/` | `inbound/v1/responses/image_budget.rs` 与 `inbound/v1/responses/tests/image_budget.rs` | 只服务 Responses 原生路径 |
| `routes/api/provider_protocol_test.rs` | `service/provider_check.rs` | 生产代码，不是测试 |
| `activity/store.rs` | `service/activity.rs` | 数据库索引与 rollout 文件的事务协调 |
| `observability/stream_stats/` | `service/recording/` | 流式统计与正文记录 |
| `observability/upstream_observability.rs` | `service/upstream_errors.rs` | 上游失败文案与日志字段 |
| `failover.rs` | `service/failover.rs` | 跨请求的供应商冷却状态 |
| `identity/cleanup.rs` | `service/cleanup.rs` | 过期身份删除流程 |
| `upstream.rs` | `outbound/client.rs` | 客户端、超时与重试策略；`AppState::provider_client` 的代理选择逻辑一并移入 |
| `extensions/` | `outbound/integrations/extensions/` | Gitea 扩展制品目录 |
| `client_update.rs`、`client_update/` | `outbound/integrations/client_update/` | 客户端安装包缓存 |
| `bridge/internal.rs` | `codec/internal.rs` | 内部规范请求与角色 |
| `bridge/usage.rs` | `codec/usage.rs` | 用量解析 |
| `bridge/` 其余 | `codec/bridge/` | 下游协议桥接与流式转换 |
| `providers/` | `codec/providers/` | 上游协议编解码、能力与地址解析、模型发现 |
| `database.rs` | `store/connect.rs` | 连接配置与连接池 |
| `schema/` | `store/schema/` | DDL 与初始化 |
| `entity/` | `store/entity/` | SeaORM 实体 |
| `storage/` | `store/storage/` | 查询实现与存储错误 |
| `activity/rollout/store.rs`、`activity/rollout/store/` | `store/rollout/` | rollout 文件介质读写 |
| `activity/` 其余 | `domain/activity/` | 事件、脱敏、媒体元数据、流捕获、录制记录类型 |
| `memory/` | `domain/memory/` | 记忆候选与规范化 |
| `provider_catalog/` | `domain/catalog/` | 供应商与模型目录 |
| `models.rs` | `domain/registry.rs` | 供应商、接入点与能力覆盖记录 |
| `stats.rs` | `domain/stats.rs` | 活动记录与统计口径 |
| `identity/credential.rs` | `domain/identity/credential.rs` | 设备凭据生成、校验与哈希 |

被测模块的私有测试跟随模块移动，断言不得减少。`tests/` 目录不重排，只更新引用路径和结构门禁。

## 依赖规则

允许的引用方向：

```text
bootstrap → inbound → service → {store, outbound} → codec → domain
```

箭头表示左侧可以依赖右侧，上层可以跨层引用更下层，但不能反向。

- `domain` 只依赖 `error` 和外部 crate。
- `codec` 只依赖 `domain`、`error`。
- `store` 只依赖 `domain`、`codec`、`error`。
- `outbound` 只依赖 `domain`、`error`。
- `service` 只依赖 `domain`、`codec`、`store`、`outbound`、`error`。
- `inbound` 只依赖上述各层、`error`。
- `bootstrap` 可以依赖所有层。
- `store` 与 `outbound` 互不依赖。

两条明确的例外，写在这里而不是靠约定：

- `domain/catalog` 读取部署时的只读配置目录。它不访问网络和数据库，读取结果只用于构造目录对象。
- `inbound` 可以直接读 `store`（查询当前身份、Provider、接入点等），但跨介质的写入必须经过 `service`。

把 `ActivityStore` 移入 `service/activity.rs`、把 rollout 文件读写移入 `store/rollout/` 后，`store` 只依赖 `domain/activity` 的记录类型，`service` 依赖 `store`，现有的双向依赖消失。

`error.rs` 不依赖任何内部层。`AppState` 是唯一允许同时持有各层类型的结构，放在 `lib.rs`。

## 根级契约

- `main.rs` 只有进程入口：加载 `.env`、初始化日志、调用 `bootstrap::serve()`。
- `lib.rs` 只有模块声明、`AppState` 和对外导出，不承载实现逻辑。
- `error.rs` 只有 `AppError` 定义、`IntoResponse` 映射和错误码映射，不引用任何层。

## 命名

| 拟用名 | 冲突来源 | 结论 |
| --- | --- | --- |
| `app` | `prelay-client/app/` 是 Nuxt 源码根目录，工作区里 app 默认指桌面端 | 不用，改 `bootstrap` |
| `protocol` | 仓库内已有 `crates/protocol`，即 `prelay-protocol` 子模块 | 不用，改 `codec` |
| `runtime` | 与 tokio runtime 同词 | 不用 |
| `store` | 客户端 `app/stores` 是 Pinia 状态 | 保留，服务端路径上下文明确 |

## 结构门禁

行数门禁保持现状：递归检查 `src/` 和 `tests/` 下所有 `.rs` 文件，超过 450 行即失败，没有白名单、没有例外，包括门禁测试自身。当前 `tests/source_layout.rs` 已有 394 行，扩展前必须先拆分：

```text
tests/
  source_layout.rs
  source_layout/
    limits.rs
    paths.rs
    dependencies.rs
```

新增门禁：

- 根目录只允许 `main.rs`、`lib.rs`、`error.rs`。
- 顶层目录只允许 `bootstrap`、`inbound`、`service`、`codec`、`outbound`、`store`、`domain`。
- 旧路径必须消失：`app.rs`、`client_update.rs`、`client_update/`、`database.rs`、`failover.rs`、`image_budget.rs`、`image_budget/`、`models.rs`、`protocol_body_limit.rs`、`stats.rs`、`upstream.rs`、`activity/`、`bridge/`、`entity/`、`extensions/`、`identity/`、`memory/`、`observability/`、`provider_catalog/`、`providers/`、`routes/`、`schema/`、`storage/`。
- 依赖方向按上文规则检查 `crate::` 引用。

依赖检查用文本扫描实现，覆盖范围和限制必须写在测试注释里：先把文件内容去掉空白，展开 `crate::{...}` 分组的首段标识符，再匹配目标层；遇到第一个 `#[cfg(test)]` 后停止扫描；宏生成的引用、`super::` 相对引用和完全限定路径不在覆盖范围。扫描出现漏报时按普通缺陷修复门禁，不靠人工约定补充。

门禁本身也要可证伪：每批迁移后故意保留一个旧路径或一条违规引用，确认测试失败，再恢复。

## 实施批次

前置条件：工作树干净。2026-10-30 已确认位于 `ede8c00`，无未提交改动。开始实施时若又有改动，先收口或确认与当前批次互不重叠。

每批只移动文件和修改路径引用，不改变行为；发现需要改行为时停止该批，单独提出。

| 批次 | 内容 | 验证 |
| --- | --- | --- |
| 1 | 拆分 `tests/source_layout.rs` 为入口加三个模块，断言与原文件等价 | `cargo test --test source_layout` |
| 2 | `domain/` 与 `codec/`：登记记录、统计、目录、活动模型、记忆、凭据、内部模型与双向编解码 | `cargo test --test provider_catalog`、`cargo test --test v1`、`cargo test --lib domain::` |
| 3 | `store/`：连接、DDL、实体、查询、rollout 文件；`From<StorageError>` 迁入 | `cargo test --test schema`、`cargo test --test identity`、`cargo test --test management` |
| 4 | `service/`：活动协调、流式记录、上游失败文案、冷却、清理、连通性检查 | `cargo test --lib service::`、`cargo test --test v1` |
| 5 | `outbound/`：客户端与重试、扩展制品、客户端更新缓存 | `cargo test --test upstream_policy`、`cargo test --test extensions` |
| 6 | `inbound/`：管理 API、协议 API、请求体上限、图片预算 | `cargo test --test management`、`cargo test --test v1` |
| 7 | `bootstrap/`：入口拆分、路由挂载、测试夹具 | `cargo build --bin prelay-server`、`cargo test --lib` |
| 8 | 门禁收敛：根文件、顶层目录、旧路径、依赖方向；同步文档中的路径说明 | `cargo test --test source_layout` 与全量检查 |

每批结束执行：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

批次表中的数据库相关测试需要 `TEST_POSTGRES_URL`。第 8 批额外执行 `cargo test --all-targets --all-features`。

## 验证与边界

- 行为不变量：`/api/*`、`/v1/*` 的路径与请求响应语义，管理 DTO，稳定错误码，数据库表名、列名、索引名、初始化顺序与 SQL 语义，活动 rollout 文件格式与目录布局，加密与凭据边界，全部不得变化。
- 每批的定向测试先于门禁执行；门禁只证明结构，不证明行为。
- 仓库 `.env` 只有 `ENCRYPTION_KEY` 和 `DATABASE_URL`，没有 `TEST_POSTGRES_URL`，当前 shell 也未设置。数据库相关测试在提供专用测试库之前无法运行；这种情况下每批只能完成编译、结构门禁和非数据库测试，最终结论必须标注全量测试未执行。
- 不启动或终止正在运行的 `prelay-server.exe`；Cargo 被文件锁阻断时如实记录，不使用额外 target 目录规避。
- 真实启动、Docker 部署和上游联调不在本次范围。

## 不纳入本次

- 任何协议、路由、DTO、错误码和数据库语义的变更。
- `ActivityStore` 内部实现拆分，只在批次 4 移动位置。
- `tests/` 目录重排，只更新引用和门禁。
- `prelay-client`、`prelay-protocol` 的运行时代码。
- 把测试支撑代码收紧为 `test-support` feature，属于独立任务。

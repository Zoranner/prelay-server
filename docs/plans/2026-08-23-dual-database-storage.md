# 双数据库存储 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** 让服务端以 DATABASE_URL 在 SQLite 或 PostgreSQL 的全新部署中运行相同的存储契约，且正常启动不执行 DDL。

**Architecture:** SeaORM 只作为存储基础设施；Storage 保持唯一领域持久化边界，AppState 只公开 Storage。迁移由独立 migrate 命令执行，服务只连接并校验迁移版本；两个数据库使用同一套实体、领域操作和契约测试。

**Tech Stack:** Rust 2021、Axum、SeaORM、SeaORM Migration、SQLite、PostgreSQL、Docker Compose、Tokio。

---

## 前置条件

- [ ] 开始实现前，确认 prelay-server 当前未提交修改已经由作者提交、舍弃或另存。身份管理路由、src/storage 和相关测试有用户改动，不能与本计划交叉编辑。
- [ ] 从干净基线建立隔离 worktree；不在协调目录或 prelay-client 中操作 Git。

~~~powershell
git -C E:\Repositories\projects\Zoranner\provider-relay\prelay-server status --short
git -C E:\Repositories\projects\Zoranner\provider-relay\prelay-server worktree add ..\prelay-server-dual-db -b feature/dual-database-storage
~~~

预期：原工作树不变，新 worktree 没有无关改动。后续命令均在新 worktree 执行。

- [ ] 记录行为基线。若 target 被用户运行的旧二进制锁定，只报告锁定路径，不终止进程，也不使用外部 target 目录规避。

~~~text
cargo test --all-targets --all-features
~~~

## 文件职责

| 文件或目录 | 职责 |
| --- | --- |
| Cargo.toml、Cargo.lock | 以 SeaORM 与迁移 crate 取代运行时 SQLx。 |
| src/database.rs | 解析 URL、建立连接、限制 SQLite 连接数、校验 schema 版本。 |
| src/migration/ | 唯一的 schema 演进和方言差异位置。 |
| src/entity/ | 九张身份范围表的 SeaORM 映射；不作为 API DTO。 |
| src/storage/ | 唯一持久化边界，包含领域事务、会话、日志和统计。 |
| src/routes/、src/bridge/、src/observability/ | 只调用 Storage，不持有连接池或 SQL。 |
| src/bin/prelay-migrate.rs | 显式执行迁移；正常服务二进制不执行 DDL。 |
| tests/storage_contract.rs | SQLite 与真实 PostgreSQL 共用的行为测试。 |
| deploy/、README.md、CI | 两种部署模式、迁移 Job 与发布门禁。 |

## 契约测试基线

**Files:**
- Create: tests/support/mod.rs
- Create: tests/storage_contract.rs
- Modify: tests/identity_storage.rs
- Modify: tests/management_isolation.rs

- [ ] **Step 1: 建立只依赖 Storage 的测试夹具。**

~~~rust
pub const TEST_MASTER_KEY: [u8; 32] = [7; 32];

pub async fn register_identity(storage: &Storage, suffix: &str) -> CreateIdentityResponse {
    storage
        .register_identity(
            &format!("machine-{suffix}"),
            &format!("S-1-5-21-{suffix}"),
            &format!("credential-{suffix}"),
        )
        .await
        .expect("register test identity")
}
~~~

- [ ] **Step 2: 写会在任一数据库运行的失败契约。** 覆盖同一 machine_id + account_sid 不能重复注册、身份 A 不能读取身份 B 的 provider 或 endpoint、无效 provider 不得成为接入点候选、供应商与模型写入的事务原子性。

~~~rust
#[tokio::test]
async fn duplicate_machine_and_sid_is_rejected() {
    let storage = test_storage().await;
    register_identity(&storage, "a").await;

    let error = storage
        .register_identity("machine-a", "S-1-5-21-a", "another-credential")
        .await
        .expect_err("duplicate identity must fail");

    assert!(matches!(error, StorageError::IdentityAlreadyRegistered));
}
~~~

- [ ] **Step 3: 先运行并确认失败。**

~~~text
cargo test --test storage_contract
~~~

预期：仅因 test_storage 与双库 harness 尚不存在而失败。

- [ ] **Step 4: 临时用 SQLite 实现 test_storage 固定 API 形状，运行定向测试后提交。**

~~~text
cargo test --test storage_contract
cargo test --test identity_storage --test management_isolation --test v1_identity_scope
git add tests
git commit -m "建立存储契约测试基线"
~~~

## 数据库连接、迁移与实体

**Files:**
- Modify: Cargo.toml、Cargo.lock、src/lib.rs
- Create: src/database.rs、src/migration/mod.rs、src/migration/m20260823_000001_initial_schema.rs
- Create: src/entity/mod.rs 和九个表实体文件
- Create: tests/database_configuration.rs、tests/migration_schema.rs

- [ ] **Step 1: 写 URL 和连接限制的失败测试。**

~~~rust
#[test]
fn rejects_missing_or_unsupported_database_url() {
    assert!(matches!(DatabaseConfig::from_url(""), Err(DatabaseConfigError::MissingUrl)));
    assert!(matches!(
        DatabaseConfig::from_url("mysql://localhost/prelay"),
        Err(DatabaseConfigError::UnsupportedScheme { .. })
    ));
}

#[test]
fn sqlite_is_single_connection_and_postgres_uses_configured_limit() {
    assert_eq!(DatabaseConfig::from_url("sqlite::memory:").unwrap().max_connections(), 1);
    assert_eq!(DatabaseConfig::from_url("postgres://user:pass@host/db").unwrap().max_connections(), 10);
}
~~~

- [ ] **Step 2: 运行失败测试。**

~~~text
cargo test --test database_configuration
~~~

预期：提示 DatabaseConfig 与相关错误类型不存在。

- [ ] **Step 3: 加入 SeaORM。** 删除运行时 SQLx 依赖；加入同版本 sea-orm 的 runtime-tokio-rustls、sqlx-sqlite、sqlx-postgres、macros feature 与同运行时、驱动的 sea-orm-migration；更新 lockfile。

- [ ] **Step 4: 实现 DatabaseConfig 和连接。** 只接受 sqlite:、postgres:、postgresql:；DATABASE_MAX_CONNECTIONS 默认 PostgreSQL 为 10，SQLite 强制为 1；连接错误日志不得包含 URL 凭据。

~~~rust
pub async fn connect(config: &DatabaseConfig) -> Result<DatabaseConnection, DatabaseError> {
    let options = ConnectOptions::new(config.url().to_owned())
        .max_connections(config.max_connections())
        .sqlx_logging(false);
    Database::connect(options).await.map_err(DatabaseError::Connect)
}
~~~

- [ ] **Step 5: 定义迁移运行器。** apply_all 只由迁移二进制调用；ensure_current 检查 pending migrations，返回 SchemaOutdated，绝不执行 DDL。

~~~rust
pub async fn ensure_current(db: &DatabaseConnection) -> Result<(), MigrationError> {
    let pending = Migrator::get_pending_migrations(db).await?;
    if pending.is_empty() {
        Ok(())
    } else {
        Err(MigrationError::SchemaOutdated { pending: pending.len() })
    }
}
~~~

- [ ] **Step 6: 用一个前向初始迁移建立完整表、外键、唯一约束和索引。** 表为 identities、identity_provider_configs、identity_provider_models、identity_endpoint_configs、identity_endpoint_models、identity_endpoint_model_routes、identity_response_sessions、identity_request_logs、identity_model_aliases。UUID API id 保持 String；时间为 RFC 3339 UTC 文本；JSON 保持 JSON 字符串；请求日志索引用普通 (identity_id, created_at)，不可保留 SQLite 北京时间表达式索引。

~~~rust
Table::create()
    .table(Identities::Table)
    .if_not_exists()
    .col(ColumnDef::new(Identities::Id).string().not_null().primary_key())
    .col(ColumnDef::new(Identities::MachineId).string().not_null())
    .col(ColumnDef::new(Identities::AccountSid).string().not_null())
    .index(Index::create().name("uq_identities_machine_sid")
        .col(Identities::MachineId).col(Identities::AccountSid).unique())
    .to_owned()
~~~

- [ ] **Step 7: 为每张表建立只含列和关系的实体。**

~~~rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "identities")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub machine_id: String,
    pub account_sid: String,
    pub credential_hash: String,
    pub display_name: String,
    pub created_at: String,
    pub last_active_at: String,
}
~~~

- [ ] **Step 8: 以空 SQLite 和真实 PostgreSQL 验证迁移与约束，提交。**

~~~powershell
cargo test --test migration_schema -- sqlite
$env:TEST_POSTGRES_URL = 'postgres://prelay_test:prelay_test@127.0.0.1:54329/prelay_test'
cargo test --test migration_schema -- postgres
git add Cargo.toml Cargo.lock src/database.rs src/migration src/entity tests
git commit -m "引入双数据库 schema 与迁移"
~~~

未设置 TEST_POSTGRES_URL 时 PG 测试必须明确 ignored，不得称已验证。

## 核心领域存储

**Files:**
- Modify: src/storage/mod.rs、src/storage/identities.rs、src/storage/providers.rs、src/storage/endpoints.rs
- Delete: src/storage/schema.rs
- Modify: tests/identity_storage.rs、tests/management_isolation.rs、tests/storage_contract.rs

- [ ] **Step 1: 将直接 SQL 测试改为 Storage 契约。** 保留身份注册、认证、轮换、显示名、失活清理、密钥加密、模型去重、endpoint token 唯一、候选顺序和跨身份拒绝；测试不得引用 Storage::pool 或表名。

- [ ] **Step 2: Storage 改为持有 DatabaseConnection。**

~~~rust
#[derive(Clone)]
pub struct Storage {
    db: DatabaseConnection,
    crypto: crypto::KeyCipher,
}

pub fn from_connection(db: DatabaseConnection, master_key: MasterKey) -> Self {
    Self { db, crypto: crypto::KeyCipher::new(master_key) }
}
~~~

移除 from_pool、pool、initialize 和所有启动 DDL。SQLite test helper 显式 apply_all 后构造 Storage；生产启动不调用它。

- [ ] **Step 3: 用 SeaORM 查询和事务替换身份 SQL。** 注册先检测同键身份并映射 IdentityAlreadyRegistered；认证成功才更新 last active 和显示名；轮换以 identity_id + credential_hash 条件更新；清理身份在一个事务中删除全部身份范围数据。

~~~rust
let result = identities::Entity::update_many()
    .col_expr(identities::Column::CredentialHash, Expr::value(new_hash))
    .filter(identities::Column::Id.eq(identity_id))
    .filter(identities::Column::CredentialHash.eq(authenticated_hash))
    .exec(&transaction)
    .await?;
if result.rows_affected != 1 {
    return Err(StorageError::InvalidCredential);
}
~~~

- [ ] **Step 4: 重写 provider 与 endpoint 操作。** create_provider 在一个事务内写入密钥、provider 和去重模型；创建或更新 endpoint 时验证所有候选 provider 属于当前身份，再原子替换 models/routes；删除 provider 要处理 models、候选、routes、aliases，使用明确删除或迁移级联，但两个数据库语义必须一致。

- [ ] **Step 5: 运行契约与隔离测试并提交。**

~~~text
cargo test --test identity_storage --test management_isolation --test storage_contract
git add src/storage tests
git commit -m "以 SeaORM 重写身份与配置存储"
~~~

## 会话、日志与统计

**Files:**
- Modify: src/storage/sessions.rs、src/storage/stats.rs、src/storage/mod.rs
- Modify: src/bridge/sessions.rs、src/observability/stream_stats.rs
- Modify: src/routes/v1/chat.rs、src/routes/v1/messages.rs、src/routes/v1/responses.rs、src/routes/management/stats.rs
- Modify: tests/v1_identity_scope.rs、tests/management_isolation.rs、tests/storage_contract.rs

- [ ] **Step 1: 写会话、流式日志和统计失败测试。** 同一 response_id 在不同 identity 互不可见；stream completion 更新一行而非另插一行；metadata、token、first-token time、工具调用数不丢失；identity B 不影响 A 的 overview、models、providers、requests、timeline。

- [ ] **Step 2: 将 session/log API 收进 Storage。** 新增 save_response_session、load_response_session_messages、insert_request_log、complete_request_log，均要求 identity_id。会话 upsert 使用先查询后 update/insert 的跨方言事务，禁止 INSERT OR REPLACE。

~~~rust
let result = request_logs::Entity::update_many()
    .set(request_logs::ActiveModel { status: Set(completion.status), ..Default::default() })
    .filter(request_logs::Column::Id.eq(log_id))
    .filter(request_logs::Column::IdentityId.eq(identity_id))
    .exec(&self.db)
    .await?;
if result.rows_affected == 0 {
    return Err(StorageError::RequestLogNotFound);
}
~~~

- [ ] **Step 3: 令桥接与流式观测仅持有 Storage。** bridge/sessions 只保留内部消息 JSON 转换；StreamStats 将 pool 参数替换为 clone 的 Storage；三个 v1 路由删除生产路径的 state.db、SqlitePool 与 sqlx query。

- [ ] **Step 4: 将管理统计收进 Storage。** 数据库只按 UTC 范围过滤聚合。北京时区 hour/day/month 边界与空桶由 Rust 生成并填充，避免 SQLite datetime、strftime、递归 CTE 与 PostgreSQL 方言差异；统计路由只保留参数和 DTO 转换。

~~~rust
let buckets = TimelineGranularity::Hour.buckets(bounds);
let by_bucket = rows.into_iter()
    .map(|row| (row.bucket_start.clone(), row))
    .collect::<HashMap<_, _>>();
Ok(buckets.into_iter()
    .map(|bucket| by_bucket.get(&bucket).cloned().unwrap_or_else(|| TokenUsageTimelineRow::empty(bucket)))
    .collect())
~~~

- [ ] **Step 5: 两个数据库验证后提交。**

~~~powershell
cargo test --test protocol_routes --test v1_identity_scope
cargo test --test storage_contract stats -- sqlite
$env:TEST_POSTGRES_URL = 'postgres://prelay_test:prelay_test@127.0.0.1:54329/prelay_test'
cargo test --test storage_contract stats -- postgres
git add src/storage src/bridge src/observability src/routes tests
git commit -m "收敛会话日志与跨数据库统计"
~~~

## 启动、部署与上线门禁

**Files:**
- Modify: src/lib.rs、src/main.rs、src/app.rs
- Create: src/bin/prelay-migrate.rs
- Delete: src/db.rs、src/routes/legacy_management/
- Modify: Dockerfile、deploy/docker-compose.yml、deploy/.env.example、README.md
- Create: deploy/docker-compose.postgres.yml
- Create or Modify: .github/workflows/ci.yml

- [ ] **Step 1: 写启动边界失败测试。**

~~~rust
#[tokio::test]
async fn service_refuses_database_with_pending_migration() {
    let db = connect(&DatabaseConfig::sqlite_memory()).await.unwrap();
    assert!(matches!(
        ensure_current(&db).await,
        Err(MigrationError::SchemaOutdated { .. })
    ));
}
~~~

另测：apply_all 后 ensure_current 成功，证明服务只校验而非执行 DDL。

- [ ] **Step 2: 从 AppState 删除裸连接，并修复所有构造点。**

~~~rust
#[derive(Clone)]
pub struct AppState {
    pub storage: storage::Storage,
    pub client: reqwest::Client,
}
~~~

- [ ] **Step 3: 用 DATABASE_URL 启动服务。** 顺序为加载 .env、验证 URL、连接、ensure_current、构造 Storage、既有身份清理、启动 HTTP。移除硬编码 sqlite:data/relay.db?mode=rwc 和 data 自动创建；缺失 URL 必须退出。

~~~rust
let config = DatabaseConfig::from_environment()?;
let db = database::connect(&config).await?;
migration::ensure_current(&db).await?;
let storage = Storage::from_connection(db, MasterKey::from_environment()?);
~~~

- [ ] **Step 4: 新增只迁移的二进制。** prelay-migrate 复用环境与连接配置，只运行 apply_all 后退出，不启动 listener、不读 provider key、不清理身份。

~~~text
cargo run --bin prelay-migrate
cargo run --bin prelay-server
~~~

预期：空库先 migrate 成功，后 service 通过 schema 检查；直接启动空库必须失败。

- [ ] **Step 5: 删除旧 SQLx/legacy 死代码前确认无引用。**

~~~text
rg -n 'crate::db|legacy_management|Storage::from_pool|Storage::initialize|state\.db|SqlitePool|sqlx::' src tests
~~~

预期：无匹配；之后删除 src/storage/schema.rs、src/db.rs 和未注册 legacy_management。不要恢复 legacy API。

- [ ] **Step 6: 更新部署。** SQLite Compose 有一次性 prelay-migrate service 与 data volume，prelay-server 依赖其成功完成。PG Compose 使用独立 postgres service、具名 volume 和同 URL 的单一 migration job，服务不挂 SQLite volume；所有口令只在未提交 .env 中，文档仅用占位符。

- [ ] **Step 7: 更新 README。** 明确必须配置 DATABASE_URL，先迁移再启动；SQLite/PG 是独立新部署，不能切换或迁移既有数据。架构文档只保留稳定决策，不复制操作步骤。

- [ ] **Step 8: 配置 CI。** SQLite job 和真实 PostgreSQL service job 都运行迁移及同一 storage_contract。禁止用 SQLite mock 代替 PG job。

- [ ] **Step 9: 最终验证并提交。**

~~~powershell
docker compose -f deploy/docker-compose.yml config
docker compose -f deploy/docker-compose.postgres.yml config
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
git add src tests deploy Dockerfile README.md .github Cargo.toml Cargo.lock
git commit -m "完成双数据库存储支持"
~~~

## 验收清单

- [ ] DATABASE_URL 是唯一数据库选择来源；未知 scheme、连接失败和 pending migration 都阻止服务启动。
- [ ] prelay-migrate 是唯一 DDL 入口；服务启动不自动迁移。
- [ ] AppState、路由、桥接、观测不再持有 pool 或拼接 SQL，所有持久化经 Storage。
- [ ] 两个数据库均从空库迁移，并通过同一身份、供应商、接入点、会话、日志、统计契约。
- [ ] 不实现跨库迁移、双写、同步、运行时切换、自动 schema 同步或泛型 CRUD repository。
- [ ] 格式、严格 Clippy、完整测试、差异检查通过；真实 PG CI 和部署迁移 Job 的结果单独作为正式上线证据。

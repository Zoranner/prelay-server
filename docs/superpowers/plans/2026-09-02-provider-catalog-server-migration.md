# 供应商与模型目录服务端迁移实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将供应商调用规则和模型白名单迁入服务端只读目录，并把现有 PostgreSQL 供应商、接入点数据事务迁移到目录驱动模型。

**Architecture:** `models.toml` 定义模型名、`text`/`image` 类型和思考档位；`provider-catalog.toml` 定义供应商认证方式、协议集合、默认 URL、协议 URL 覆盖及可提供模型。数据库只保留供应商接入的名称、目录供应商 ID、URL 覆盖和加密 API Key；运行时由服务端目录解析认证、协议和模型。迁移保留 Endpoint Token、接入点模型名和候选顺序，不实现旧管理客户端兼容层。

**Tech Stack:** Rust 2021、Axum、SeaORM 2、Sea Query、PostgreSQL、SQLite 测试库、Serde、TOML、prelay-protocol。

**Spec:** `docs/architecture/provider-catalog.md`

## Global Constraints

- 本阶段只修改 `prelay-protocol` 与 `prelay-server`；不得修改 `prelay-client`，其当前未提交改动必须保留。
- 不保留旧管理 DTO 或 `provider_type` 运行时兼容层；旧客户端失去管理能力是预期结果。
- `/v1/models`、`/v1/responses`、`/v1/chat/completions`、`/v1/messages` 的路径和 Endpoint Token 不变。
- 协议枚举统一为 `chat_completions`、`responses`、`anthropic_messages`、`images_generations`，配置和 API 输出固定按此顺序排列。
- 模型只声明 `model_type = "text"` 或 `model_type = "image"`；模型配置不声明协议。
- 服务端目录文件为 `/app/config/models.toml` 与 `/app/config/provider-catalog.toml`；本地与测试通过 `PRELAY_CATALOG_DIR` 指定目录。
- 已有 PostgreSQL 数据只在全部引用能精确映射到目录供应商和模型时迁移；不猜测旧 `upstream_model` 别名或自定义供应商。
- Rust 修改后执行 `cargo fmt --all`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features` 和 `git diff --check`。

---

## 文件结构与责任

| 仓库 | 文件 | 责任 |
| --- | --- | --- |
| `prelay-protocol` | `src/providers.rs` | 目录、供应商接入、协议 URL 覆盖和模型类型 DTO。 |
| `prelay-protocol` | `src/endpoints.rs` | 接入点候选改为 `provider_id + model_id`，不再传递 `upstream_model`。 |
| `prelay-server` | `src/provider_catalog.rs` | 加载、解析、排序和校验两个 TOML 目录文件。 |
| `prelay-server` | `src/schema/mod.rs`、`src/schema/provider_catalog.rs` | 空库建表、PostgreSQL 版本迁移和迁移前校验。 |
| `prelay-server` | `src/entity/identity/provider_configs.rs`、`endpoint_models.rs` | 新数据库字段映射。 |
| `prelay-server` | `src/storage/providers.rs`、`endpoints.rs`、`access.rs` | 目录约束下保存供应商、接入点候选和协议调用数据。 |
| `prelay-server` | `src/providers/spec/*`、`src/routes/v1/*` | 按目录供应商解析认证、URL、固定协议选择和模型类型。 |
| `prelay-server` | `src/routes/api/provider_catalog.rs`、`src/routes/api/providers.rs` | 只读目录 API 与新的供应商管理 API。 |
| `prelay-server` | `config/models.toml`、`config/provider-catalog.toml` | 不含密钥的默认目录；部署可用 `/app/config` 挂载覆盖。 |
| `prelay-server` | `tests/provider_catalog.rs`、`tests/provider_catalog_postgres.rs`、`tests/schema/*` | 目录校验、PostgreSQL 迁移、管理 API 和 `/v1` 回归。 |

## Task 1: 建立管理协议目录契约

**Files:**

- Modify: `prelay-protocol/src/providers.rs`
- Modify: `prelay-protocol/src/endpoints.rs`
- Modify: `prelay-protocol/src/lib.rs`
- Modify: `prelay-protocol/tests/management_dto.rs`

**Interfaces:**

- Produces `ProviderProtocol`、`ModelType`、`ProviderCatalogResponse`、`CatalogModelResponse`、`CatalogProviderResponse`、`ProviderProtocolBaseUrl`.
- Replaces旧 `CreateProviderRequest`、`UpdateProviderRequest`、`ProviderResponse` 和 `EndpointModelInput` 的可自由配置模型字段。

- [ ] **Step 1: 写目录 DTO 的序列化失败测试**

  在 `management_dto.rs` 增加以下断言：

  ```rust
  assert_json_round_trip(ProviderCatalogResponse {
      models: vec![CatalogModelResponse {
          id: "gpt-image-1".to_string(),
          display_name: "GPT Image 1".to_string(),
          model_type: ModelType::Image,
          reasoning_efforts: Vec::new(),
          default_reasoning_effort: None,
      }],
      providers: vec![CatalogProviderResponse {
          id: "gotoken".to_string(),
          name: "GoToken 套餐".to_string(),
          auth_scheme: ProviderAuthScheme::Bearer,
          base_url: "https://gotoken.cc".to_string(),
          protocols: vec![
              ProviderProtocol::ChatCompletions,
              ProviderProtocol::Responses,
              ProviderProtocol::AnthropicMessages,
              ProviderProtocol::ImagesGenerations,
          ],
          protocol_base_urls: vec![ProviderProtocolBaseUrl {
              protocol: ProviderProtocol::ImagesGenerations,
              base_url: "https://gotoken.cc/v1".to_string(),
          }],
          models: vec!["gpt-image-1".to_string()],
      }],
  });
  ```

- [ ] **Step 2: 运行 DTO 测试确认缺失类型导致失败**

  Run:

  ```text
  cargo test -p prelay-protocol management_dto
  ```

  Expected: 编译失败，提示目录 DTO、协议枚举或模型类型尚未定义。

- [ ] **Step 3: 实现类型化协议与新管理 DTO**

  在 `providers.rs` 定义：

  ```rust
  #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
  #[serde(rename_all = "snake_case")]
  pub enum ProviderProtocol {
      ChatCompletions,
      Responses,
      AnthropicMessages,
      ImagesGenerations,
  }

  impl ProviderProtocol {
      pub const ORDERED: [Self; 4] = [
          Self::ChatCompletions,
          Self::Responses,
          Self::AnthropicMessages,
          Self::ImagesGenerations,
      ];
  }
  ```

  定义 `ModelType::{Text, Image}`、`ProviderAuthScheme::{Bearer, Anthropic}`、目录响应 DTO 和协议 URL 项。将供应商创建/更新请求收敛为名称、`catalog_provider_id`、`base_url`、有序协议 URL 覆盖和 API Key；删除模型、能力覆盖和 `provider_type` 输入。将接入点候选收敛为 `provider_id + model_id`，响应同时返回模型类型和思考档位。

- [ ] **Step 4: 更新协议导出并运行协议测试**

  Run:

  ```text
  cargo test -p prelay-protocol
  ```

  Expected: 所有管理 DTO round-trip 测试通过；JSON 中没有 `provider_type`、`capabilities`、`upstream_model` 或模型发现结果字段。

- [ ] **Step 5: 提交协议契约**

  ```text
  git -C prelay-protocol add src/providers.rs src/endpoints.rs src/lib.rs tests/management_dto.rs
  git -C prelay-protocol commit -m "定义供应商目录管理协议"
  ```

## Task 2: 加载并校验服务端模型与供应商目录

**Files:**

- Modify: `prelay-server/Cargo.toml`
- Create: `prelay-server/src/provider_catalog.rs`
- Modify: `prelay-server/src/lib.rs`
- Modify: `prelay-server/src/main.rs`
- Modify: `prelay-server/src/routes/api/mod.rs`
- Create: `prelay-server/src/routes/api/provider_catalog.rs`
- Create: `prelay-server/config/models.toml`
- Create: `prelay-server/config/provider-catalog.toml`
- Create: `prelay-server/tests/provider_catalog.rs`

**Interfaces:**

- Produces `ProviderCatalog::load(directory: &Path) -> Result<ProviderCatalog, ProviderCatalogError>`.
- Produces `ProviderCatalog::model(&self, model_id: &str) -> Option<&CatalogModel>` and `ProviderCatalog::provider(&self, catalog_provider_id: &str) -> Option<&CatalogProvider>`.
- Adds `catalog: Arc<ProviderCatalog>` to `AppState`.
- Exposes authenticated `GET /api/provider-catalog`.

- [ ] **Step 1: 写目录加载与顺序校验测试**

  在 `tests/provider_catalog.rs` 用临时目录写入下列最小文件：

  ```toml
  # models.toml
  [[models]]
  id = "gpt-image-1"
  display_name = "GPT Image 1"
  model_type = "image"
  reasoning_efforts = []
  ```

  ```toml
  # provider-catalog.toml
  [[providers]]
  id = "gotoken"
  name = "GoToken 套餐"
  auth_scheme = "bearer"
  base_url = "https://gotoken.cc"
  protocols = ["chat_completions", "responses", "anthropic_messages", "images_generations"]
  models = ["gpt-image-1"]
  ```

  覆盖：重复模型 ID、供应商引用未知模型、协议顺序错误、`image` 模型存在默认思考档位、`text` 模型的默认档位不在列表中。

- [ ] **Step 2: 运行加载测试确认失败**

  Run:

  ```text
  cargo test --test provider_catalog
  ```

  Expected: 编译失败，提示 `provider_catalog` 模块或加载接口不存在。

- [ ] **Step 3: 实现目录加载、排序和严格校验**

  加入 `toml` 依赖。`ProviderCatalog::load` 从 `PRELAY_CATALOG_DIR` 指向目录读取 `models.toml` 与 `provider-catalog.toml`，默认目录为 `config`。解析后：

  - 将模型和供应商 ID 放入 `BTreeMap`，拒绝重复 ID。
  - 将每个 `protocols` 数组与 `ProviderProtocol::ORDERED` 的非空子序列比较，拒绝乱序或重复。
  - 校验所有 `protocol_base_urls` 键属于该供应商协议集合，URL 非空。
  - 校验每个供应商模型引用存在。
  - 校验 `text` 模型的思考档位为允许值，默认值仅在非空列表中出现；`image` 模型必须没有默认值且思考档位为空。

  将目录装入 `AppState`。`GET /api/provider-catalog` 只返回目录 DTO，不读取或暴露数据库 API Key。

- [ ] **Step 4: 增加默认目录和启动测试**

  `config/models.toml` 与 `config/provider-catalog.toml` 写入设计文档中当前的 GoToken、DeepSeek、GPT-5.6 和 DeepSeek V4、GPT Image 条目。更新 `main.rs`：在连接数据库前加载目录，目录错误直接终止启动。更新 `test_support::test_state`，使用测试目录创建 `AppState`。

  Run:

  ```text
  cargo test --test provider_catalog
  cargo test --test database_configuration
  ```

  Expected: 临时目录有效配置可加载；任一无效配置在服务启动前失败。

- [ ] **Step 5: 提交目录加载与只读 API**

  ```text
  git -C prelay-server add Cargo.toml Cargo.lock config src/provider_catalog.rs src/lib.rs src/main.rs src/routes/api/mod.rs src/routes/api/provider_catalog.rs tests/provider_catalog.rs
  git -C prelay-server commit -m "加载服务端供应商与模型目录"
  ```

## Task 3: 为 PostgreSQL 引入目录迁移并重建存储模型

**Files:**

- Create: `prelay-server/src/schema/provider_catalog.rs`
- Modify: `prelay-server/src/schema/mod.rs`
- Modify: `prelay-server/src/schema/tables/providers.rs`
- Modify: `prelay-server/src/schema/tables/endpoints.rs`
- Modify: `prelay-server/src/entity/identity/provider_configs.rs`
- Modify: `prelay-server/src/entity/identity/endpoint_models.rs`
- Modify: `prelay-server/src/entity/identity/mod.rs`
- Modify: `prelay-server/src/storage/providers.rs`
- Modify: `prelay-server/src/storage/endpoints.rs`
- Modify: `prelay-server/src/storage/access.rs`
- Create: `prelay-server/tests/provider_catalog_postgres.rs`
- Modify: `prelay-server/tests/schema/initialization.rs`

**Interfaces:**

- Replaces `identity_provider_configs.provider_type` with `catalog_provider_id`.
- Replaces `capabilities_json` with `protocol_base_urls_json`.
- Removes `identity_provider_models`、`identity_model_aliases` 和 `identity_endpoint_models.upstream_model`.
- Changes `schema::initialize` to `initialize(db: &DatabaseConnection, catalog: &ProviderCatalog)`.

- [ ] **Step 1: 写 PostgreSQL 迁移前置校验测试**

  在 `provider_catalog_postgres.rs` 使用 `TEST_POSTGRES_URL` 指向独立测试库，建立旧表并插入：

  ```text
  provider_type = gotoken
  provider model = gpt-5.6-luna
  endpoint model_name = gpt-5.6-luna
  endpoint upstream_model = gpt-5.6-luna
  ```

  断言迁移后：

  ```text
  identity_provider_configs.catalog_provider_id = gotoken
  identity_endpoint_models 不再有 upstream_model 列
  endpoint token 不变
  candidate_order 不变
  ```

  再插入 `model_name != upstream_model` 的旧映射，断言迁移拒绝并在错误中列出 endpoint、provider、两个模型名。

- [ ] **Step 2: 运行 PostgreSQL 迁移测试确认失败**

  Run:

  ```text
  $env:TEST_POSTGRES_URL = "postgres://prelay_test:prelay_test@127.0.0.1:5432/prelay_test"
  cargo test --test provider_catalog_postgres
  ```

  Expected: 编译失败，提示目录迁移入口不存在。不得使用生产数据库或运行中部署数据库。

- [ ] **Step 3: 实现版本化 PostgreSQL 迁移**

  新增 `prelay_schema_migrations` 表，记录固定版本 `provider_catalog_v1`。仅当数据库后端为 PostgreSQL 且版本未记录时执行单事务迁移：

  1. 查询所有旧供应商，使用一次性映射 `gotoken -> gotoken`、`kimi_coding_anthropic -> kimi_code`、`zhipu_coding -> bigmodel_coding_plan`、`minimax_token -> minimax_token_plan`、`deepseek -> deepseek`、`qwen -> bailian`、`kimi -> kimi`、`zhipu -> bigmodel`、`minimax -> minimax`。
  2. 对每条映射验证目录供应商存在；将旧 `capabilities_json.protocol_base_urls.openai`、`.anthropic` 改名为 `chat_completions`、`anthropic_messages` 后写入新的 `protocol_base_urls_json`。
  3. 验证旧 `identity_provider_models` 中每个模型均被目录供应商声明。
  4. 验证旧接入点候选的 `model_name == upstream_model`，并且该模型被候选供应商目录声明。
  5. 仅在全部验证成功后新增 `catalog_provider_id`、`protocol_base_urls_json`，回填数据，删除 `provider_type`、`capabilities_json`、`upstream_model`，删除 `identity_provider_models` 与 `identity_model_aliases`，写入版本记录。

  未识别旧 `provider_type`、目录缺失模型或旧别名均返回迁移错误并回滚。SQLite 只支持创建新目录结构；已有 SQLite 旧结构明确报错，不进行升级。

- [ ] **Step 4: 改写实体和存储**

  `ProviderConfig` 与 provider entity 使用 `catalog_provider_id`、`base_url`、`protocol_base_urls_json`、密文 API Key。供应商创建/更新通过目录验证 `catalog_provider_id` 和 URL 覆盖键；不再写入供应商模型表、能力 JSON 或类型。

  接入点候选只保存 `model_name`、`provider_id`、`candidate_order`。`validate_models` 通过目录验证模型存在且目录供应商允许该模型，不再查询 `identity_provider_models` 或比较 `upstream_model`。

- [ ] **Step 5: 运行 SQLite 空库与 PostgreSQL 迁移测试**

  Run:

  ```text
  cargo test --test schema_initialization
  $env:TEST_POSTGRES_URL = "postgres://prelay_test:prelay_test@127.0.0.1:5432/prelay_test"
  cargo test --test provider_catalog_postgres
  ```

  Expected: SQLite 空库创建新结构；PostgreSQL 旧库仅在所有记录可精确映射时事务迁移；失败场景没有部分 DDL 或部分数据写入。

- [ ] **Step 6: 提交 PostgreSQL 迁移与存储转换**

  ```text
  git -C prelay-server add src/schema src/entity/identity src/storage tests/schema
  git -C prelay-server commit -m "迁移供应商配置到服务端目录"
  ```

## Task 4: 用目录替换上游解析与管理路由

**Files:**

- Modify: `prelay-server/src/models.rs`
- Modify: `prelay-server/src/providers/spec/capabilities.rs`
- Modify: `prelay-server/src/providers/spec/urls.rs`
- Delete: `prelay-server/src/providers/model_discovery.rs`
- Modify: `prelay-server/src/providers/mod.rs`
- Modify: `prelay-server/src/routes/api/providers.rs`
- Modify: `prelay-server/src/routes/v1/endpoint_resolver.rs`
- Modify: `prelay-server/src/routes/v1/models.rs`
- Modify: `prelay-server/src/routes/v1/images/*`
- Modify: `prelay-server/src/routes/v1/responses/*`
- Modify: `prelay-server/src/routes/v1/chat/*`
- Modify: `prelay-server/src/routes/v1/messages/*`
- Modify: `prelay-server/tests/provider_catalog.rs`

**Interfaces:**

- `ProviderCatalog::resolve_provider(catalog_provider_id, base_url, protocol_base_urls)` returns effective authentication, effective URL and supported protocol set.
- `resolve_endpoint_model_candidates` receives catalog state and rejects a text request for an `image` model or an image request for a `text` model.
- `POST /api/providers/discover-models` is removed.

- [ ] **Step 1: 写候选解析失败测试**

  在 `tests/provider_catalog.rs` 建立目录驱动的供应商和接入点，断言：

  ```rust
  assert!(resolve_endpoint_model_candidates(&state, &access, "gpt-image-1", "responses")
      .await
      .is_err());
  assert!(resolve_endpoint_model_candidates(
      &state,
      &access,
      "gpt-image-1",
      "images_generations",
  )
  .await
  .is_ok());
  ```

  对文本模型反向断言 Images Generations 入口拒绝。为 GoToken `anthropic_messages` 候选断言使用目录中的 Bearer 认证和协议 URL。

- [ ] **Step 2: 运行定向测试确认失败**

  Run:

  ```text
  cargo test --test provider_catalog
  ```

  Expected: 失败，因为当前 resolver 仍依赖 `ProviderSpec::from_provider_config`、`upstream_model` 和旧能力覆盖。

- [ ] **Step 3: 实现目录驱动的协议解析**

  删除 `ProviderSpec::from_provider_config` 的 `provider_type` 分支、URL 特例和模型发现回退。以目录供应商的 `auth_scheme`、协议集合、`base_url` 与 `protocol_base_urls` 构造运行时请求规则。

  固定下游选择顺序保持：

  ```text
  responses: responses -> chat_completions -> anthropic_messages
  chat_completions: chat_completions
  anthropic_messages: anthropic_messages -> chat_completions -> responses
  images_generations: images_generations
  ```

  删除模型发现路由、DTO 调用和测试。`/v1/models` 从接入点候选与模型目录组合输出模型类型；不再输出旧供应商能力覆盖或上游别名。

- [ ] **Step 4: 改写供应商管理 API**

  `POST`、`PUT`、`GET /api/providers` 只处理新 DTO。协议测试请求通过 `catalog_provider_id` 查目录规则，允许测试用户输入的 URL 覆盖和 API Key，但不接受自由协议、模型或认证方式。目录外 URL 键、目录外模型和未知目录供应商返回明确 `BadRequest`。

- [ ] **Step 5: 运行服务端路由回归**

  Run:

  ```text
  cargo test --test provider_catalog
  cargo test --all-targets provider_catalog
  ```

  Expected: 文本和图像模型只进入对应调用链；现有 Endpoint Token 与候选顺序保持；任何 `/v1` 请求都不依赖旧桌面客户端。

- [ ] **Step 6: 提交目录驱动调用链**

  ```text
  git -C prelay-server add src/models.rs src/providers src/routes/api src/routes/v1 tests/provider_catalog.rs
  git -C prelay-server commit -m "按供应商目录解析上游调用"
  ```

## Task 5: 服务端部署材料与最终验证

**Files:**

- Modify: `prelay-server/Dockerfile`
- Modify: `prelay-server/deploy/docker-compose.yml`
- Modify: `prelay-server/README.md`
- Modify: `prelay-server/docs/architecture/provider-catalog.md`
- Modify: `prelay-server/docs/architecture/database.md`
- Modify: `prelay-server/tests/schema/contract.rs`

**Interfaces:**

- Docker 容器从只读 `/app/config/models.toml` 与 `/app/config/provider-catalog.toml` 读取目录。
- PostgreSQL 迁移完成后保留 Endpoint Token、接入点模型名和候选顺序。

- [ ] **Step 1: 写部署与 schema 契约失败测试**

  在 `tests/schema/contract.rs` 断言新列存在、旧列和旧表不存在：

  ```text
  identity_provider_configs.catalog_provider_id
  identity_provider_configs.protocol_base_urls_json
  identity_endpoint_models.model_name
  ```

  并断言不存在：

  ```text
  identity_provider_configs.provider_type
  identity_provider_configs.capabilities_json
  identity_endpoint_models.upstream_model
  identity_provider_models
  identity_model_aliases
  ```

- [ ] **Step 2: 运行 schema 契约测试确认失败**

  Run:

  ```text
  cargo test --test schema_contract
  ```

  Expected: 旧 schema 仍存在时断言失败。

- [ ] **Step 3: 更新部署与运行文档**

  Dockerfile 创建 `/app/config`；Compose 继续以只读方式挂载 `./app/config:/app/config:ro`。README 说明部署必须同时提供 `models.toml`、`provider-catalog.toml`，修改后需重启，并明确旧桌面客户端不能管理新服务端。

  文档删除“新库部署是唯一方式”的表述，改为：当前 PostgreSQL 可执行受目录校验保护的版本化迁移；SQLite 旧库不升级。

- [ ] **Step 4: 执行完整 Rust 验证**

  Run:

  ```text
  cargo fmt --all
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-targets --all-features
  git diff --check
  ```

  再在独立 PostgreSQL 测试库执行：

  ```text
  $env:TEST_POSTGRES_URL = "postgres://prelay_test:prelay_test@127.0.0.1:5432/prelay_test"
  cargo test --test provider_catalog_postgres
  ```

  Expected: 本地 Rust 门禁全部通过；PostgreSQL 测试仅证明独立测试库迁移，不代表生产库或真实上游已验收。

- [ ] **Step 5: 按仓库边界提交**

  ```text
  git -C prelay-server add Dockerfile deploy/docker-compose.yml README.md docs/architecture tests/schema tests/identity
  git -C prelay-server commit -m "支持服务端供应商与模型目录迁移"
  ```

## 计划自审

- 目录配置、类型化 DTO、PostgreSQL 迁移、运行时上游解析、管理 API、`/v1` 兼容边界、模型类型、文生图、部署和测试均有对应任务。
- 计划不包含 `prelay-client` 文件修改；其当前未提交改动不在本阶段范围。
- 旧客户端不兼容仅影响管理面；Task 3 和 Task 4 明确保留 Endpoint Token、接入点模型名、候选顺序和 `/v1/*` 路径。
- 不存在 `TODO`、`TBD`、自由模型、自由协议、自由认证方式或自动模型发现步骤。

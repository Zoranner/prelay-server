# 目录接口与配置结构统一实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task with review checkpoints.

**Goal:** 将供应商与模型静态目录统一到 `catalog/` 配置目录和 `/api/catalog/*` 管理接口，同时保持 `/v1/*` 只承载标准协议。

**Architecture:** 服务启动时从 `PRELAY_CATALOG_DIR` 指向的目录读取 `providers.toml` 与 `models/` 下的两类模型文件，运行时由单一目录对象提供供应商和模型查询。管理 API 使用 `/api/catalog/providers` 与 `/api/catalog/models/{language|image-generation}`，协议 API 不暴露自定义目录字段或非标准模型列表路径。

**Tech Stack:** Rust 2021、Axum 0.7、Serde、TOML、SeaORM。

**Spec:** `docs/architecture/provider-catalog.md`

## 全局约束

- 只修改 `prelay-server`，不修改客户端或协议仓，除非当前服务端编译契约确实要求同步。
- SQLite 仅用于调试和测试，不设计迁移。
- 不提交密钥、数据库内容或运行时产物。
- 配置文件不新增测试；接口和加载行为使用已有测试入口验证。
- 修改 Rust 后执行 `cargo fmt --all`、Clippy、受影响测试和 `git diff --check`。
- 分阶段提交，每个提交只包含一个逻辑模块。

### 阶段一：配置目录与目录加载器

**文件：** `deploy/app/config/catalog/providers.toml`、`deploy/app/config/catalog/models/*`、`src/main.rs`、`src/lib.rs`、`src/provider_catalog.rs`、Docker/Compose/架构文档。

- 移动配置文件到 `config/catalog/providers.toml` 和 `config/catalog/models/`。
- 让默认路径和 `PRELAY_CATALOG_DIR` 都指向 catalog 根目录。
- 保留模型类别分离和固定字段顺序，删除旧路径引用。
- 运行目录加载和受影响测试，提交配置结构阶段。

### 阶段二：统一 `/api/catalog/*`

**文件：** `src/routes/api/provider_catalog.rs`、`src/routes/api/mod.rs`、目录 DTO 使用处、`tests/management/provider_catalog.rs`、接口文档。

- 将路由模块重命名为 `catalog`，挂载供应商目录列表/详情和语言/图像生成模型列表/详情。
- 目录查询响应只读取启动时加载的静态目录，不暴露密钥或身份数据。
- 删除旧 `/api/provider-catalog` 路由及其文档引用。
- 为每个列表和详情路由补充回归验证，提交 API 目录阶段。

### 阶段三：标准 `/v1` 边界

**文件：** `src/routes/v1/models.rs`、`src/routes/v1/images/models.rs`、`src/routes/v1/images/mod.rs`、相关测试和架构文档。

- `/v1/models` 只返回标准模型对象字段，不返回 Prelay 专有字段。
- 删除未挂载且非官方标准的 `/v1/images/models` 处理函数、文档和测试引用。
- 修复当前模型列表回归夹具，使其使用目录中的真实模型。
- 运行 `/v1` 路由与全量测试，提交协议边界阶段。

### 阶段四：文档与部署收口

**文件：** `docs/architecture/provider-catalog.md`、`README.md`、部署清单和评审报告。

- 统一术语为 catalog、providers、language models、image-generation models。
- 补齐 API 矩阵、认证边界、配置目录和重启生效说明。
- 执行完整 Rust 验证，检查工作树并按逻辑阶段提交。

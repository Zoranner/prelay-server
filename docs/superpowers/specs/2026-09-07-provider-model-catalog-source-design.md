# 供应商模型目录唯一来源设计

## 目标

让服务端目录成为供应商模型支持关系的唯一事实来源，删除用户供应商模型副本 `identity_provider_models`，使目录新增模型自动对已有供应商生效，同时保留接入点对外模型映射和多供应商路由能力。

## 当前问题

当前系统同时保存两份供应商模型事实：

- `prelay-server/config/catalog/providers.toml` 保存目录供应商支持的模型 ID。
- `identity_provider_models` 保存每个用户供应商已保存的模型 ID。

供应商管理接口读取后者，接入点保存和运行时解析也依赖后者。因此目录新增模型不会自动出现在既有供应商中，模型目录与用户数据会发生漂移。

## 目标边界

### 保留

- `identity_provider_configs`：保存用户供应商的连接配置、加密 API Key 和能力覆盖。
- `identity_endpoint_configs`：保存用户接入点和 Endpoint Token。
- `identity_endpoint_models`：保存接入点实际暴露的模型、供应商和上游模型映射。
- `identity_endpoint_model_routes`：保存同一接入点模型组的当前路由选择。
- `ProviderCatalog`：保存部署目录中的模型详情、供应商关系、协议和显示信息。
- `/api/providers/discover-models`：继续作为上游诊断接口，不产生持久化模型事实。

### 删除

- `identity_provider_models` 表及其 SeaORM 实体、索引、模块注册和迁移清理逻辑。
- `ProviderResponse.models` 及 `ProviderModelResponse`，避免用目录投影伪装成用户数据库模型行。
- 创建和更新供应商请求中的 `models` 字段。
- 客户端供应商模型副本及供应商保存时的模型数组。

## 目标模型

供应商支持模型统一由以下关系推导：

```text
identity_provider_configs.provider_type
        |
        v
ProviderCatalog.providers[provider_type]
        |
        v
language_models / image_generation_models
```

目录中的模型 ID 是内部保存、校验、筛选和路由使用的值；显示名称和能力字段仍从目录投影，不写回数据库。

接入点模型仍是独立事实：

```text
identity_endpoint_models
  - endpoint_id
  - model_name
  - provider_id
  - upstream_model
  - candidate_order
```

目录新增模型不会自动加入已有接入点，也不会自动成为正式 API 的对外模型。用户仍需在接入点中显式建立映射。

## 服务端设计

### 供应商响应

`ProviderResponse` 只返回供应商连接配置、能力覆盖和基础身份信息，不再返回 `models`。调用方需要使用同一份 `ProviderCatalogResponse` 按 `provider_type` 获取供应商模型。

管理 API 的 `/api/providers` 和 `/api/catalog` 保持独立：

- `/api/providers` 返回用户保存的供应商连接资源。
- `/api/catalog` 返回部署目录和模型能力。

### 供应商写入

创建和更新供应商只保存连接配置。服务端验证 `provider_type` 必须存在于目录；不再接收或写入供应商模型数组。

现有供应商更新不再替换模型行，也不因目录变更执行用户数据写入。目录加载成功后，所有相同 `provider_type` 的供应商立即拥有目录声明的模型集合。

### 接入点校验

创建或更新接入点时：

1. 校验 `provider_id` 属于当前 identity。
2. 根据该供应商的 `provider_type` 查询 `ProviderCatalog`。
3. 校验 `upstream_model` 是该目录供应商支持的语言模型或图像模型。
4. 通过后写入 `identity_endpoint_models`。

不再通过 `identity_provider_models` 判断供应商是否支持模型。

### 运行时解析

运行时解析接入点模型时，继续读取 `identity_endpoint_models` 和 `identity_provider_configs`。模型有效性由接入点写入时的目录校验和 PostgreSQL 启动迁移保证；正式 `/v1/models` 额外过滤目录外语言模型。

正式 `/v1/models` 仍只列接入点已配置且目录有效的语言模型，不列出供应商目录中的全部模型。

### 数据迁移

新增一次性目录模型来源迁移，仅适用于 PostgreSQL：

1. 读取现有供应商配置并确认 `provider_type` 能唯一映射到目录供应商。
2. 删除接入点中不再存在于目录供应商关系中的模型映射及对应路由。
3. 删除目录外的模型别名。
4. 删除 `identity_provider_models` 表。
5. 记录迁移版本，保证重复启动幂等。

有效的 `identity_endpoint_models` 保留。无法映射到目录的历史供应商配置不隐式降级为自定义模型，而是使启动失败并报告明确的迁移错误，避免丢失模型事实或形成第二套模型来源。

## 客户端设计

### 协议类型

协议仓库删除供应商请求中的 `models` 字段、`ProviderResponse.models` 和 `ProviderModelResponse`。供应商保存命令只发送连接配置和能力覆盖。

### 供应商页面

供应商页面通过全局模型目录按 `provider_type` 计算模型清单和模型数量。编辑已有供应商时不读取旧模型数组，因此目录新增模型会直接显示。

### 接入点页面

接入点模型选项通过供应商的 `provider_type` 查询目录模型。接入点提交仍只发送：

```text
provider_id
upstream_model
```

接入点页面的模型组、同组多供应商映射和重复映射限制保持不变。

## 错误与兼容边界

- 目录不存在的 `provider_type`：供应商创建、更新和启动迁移均返回稳定校验或迁移错误。
- 目录不存在的 `upstream_model`：接入点创建和更新返回模型不受支持错误。
- 目录运行时移除模型：该模型不再出现在 `/v1/models`，对应正式请求无法解析。
- 上游 `/models` 发现结果：只作为诊断结果返回，不写入目录或用户数据库。
- 已有旧客户端发送 `models` 字段：服务端在过渡期忽略未知字段；新客户端不再发送该字段。正式协议类型和客户端实现统一后删除兼容测试。

## 验证

### 服务端

- 目录供应商模型集合测试。
- 已有供应商在目录增加模型后返回连接资源仍可用，且模型由目录查询得到。
- 接入点可使用目录模型，不依赖 `identity_provider_models`。
- 目录外接入点模型被拒绝或在运行时过滤。
- `/v1/models` 只返回有效接入点模型。
- PostgreSQL 迁移测试覆盖清理、删除旧表和幂等执行；SQLite 仅验证全新 schema 不创建旧表。
- 全量引用扫描确认生产代码不再引用 `identity_provider_models`。

### 协议与客户端

- DTO 序列化不再包含供应商模型数组。
- 供应商保存命令不再提交 `models`。
- 供应商和接入点页面从目录推导模型选项。
- 目录新增模型后既有供应商页面显示新模型。
- 客户端 Bun 测试、类型检查和构建通过。

## 不在本次范围

- 不自动把目录新增模型加入已有接入点。
- 不自动调用上游 `/models` 修改目录配置。
- 不改变 Endpoint Token、Provider API Key 或 identity 隔离规则。
- 不修改当前 `prelay-client` 工作树中与本设计无关的未提交推理配置。

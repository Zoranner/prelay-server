# Provider 分享与使用统计设计

## 目标

允许同一个 `prelay-server` 实例内的身份分享已配置的 Provider。被分享用户可以在供应商列表中看到该 Provider，将它引用到自己的 Endpoint，并查看该 Provider 的完整使用统计。

本期不实现配额、计费、限流、分享链接、跨服务分享或组织层级。

## 已确认规则

- 分享范围支持三种状态：
  - `private`：仅创建人可见。
  - `selected`：仅指定身份可见。
  - `all`：当前服务实例内所有已注册身份可见。
- Provider 的创建人是其原始 `identity_id`，供应商列表增加创建人展示。
- 可见用户可以查看 Provider 的非敏感元数据，并将其引用到自己的 Endpoint。
- 非创建人不能编辑、删除或测试该 Provider，不能查看、复制或接收 API Key。
- Provider API Key 始终由服务端使用 `ENCRYPTION_KEY` 加密保存，并只在服务端实际调用上游时解密。
- 创建人关闭分享、移除指定用户或删除 Provider 后，其他身份对该 Provider 的引用立即失效。
- 已产生的请求统计不因分享撤销而删除。
- 所有能看到 Provider 的用户都可以查看该 Provider 的完整使用统计，包括各使用者明细；不再按创建人和使用者区分统计可见性。
- 创建人可以继续管理分享范围和自身 Provider 配置。

## 方案

采用“Provider 原实体 + 共享授权关系”方案，不复制 Provider 配置或密钥。

现有 `identity_provider_configs` 继续作为 Provider 的唯一实体，保留其原始 `identity_id`。新增共享授权数据表达 Provider 的可见范围和指定身份。Endpoint 模型仍保存原 Provider ID，不保存副本。

读取 Provider、保存 Endpoint 和协议路由解析都必须经过统一的可见性判断：

```text
当前身份
  -> 可见 Provider 查询
      -> 自有 Provider
      -> private 不扩展
      -> selected 且存在当前身份授权
      -> all 且 Provider 仍处于分享状态
```

不能通过只修改供应商列表查询的方式实现分享，否则接收方虽然能看到 Provider，却可能在 Endpoint 保存或协议调用时绕过授权。

## 数据模型

### Provider 分享状态

可以在 Provider 表中保存当前范围：

- `visibility`: `private`、`selected` 或 `all`
- `updated_at`：用于审计和缓存失效判断，可按现有时间字段风格实现

`selected` 状态的身份关系单独保存，建议表名为 `identity_provider_shares`：

- `provider_id`
- `grantee_identity_id`
- `created_at`

约束：

- `provider_id + grantee_identity_id` 唯一。
- Provider 删除时级联删除分享关系。
- 被授权身份删除时级联删除其授权关系。
- 创建人不能把自己作为额外授权对象；创建人始终拥有自己的 Provider。
- `private` 和 `all` 状态不需要创建明细授权行。

如果后续需要撤销审计，可增加撤销历史表；本期只要求当前授权状态和历史使用统计，不增加独立审计实体。

### Endpoint 引用

现有 Endpoint 模型映射继续使用 `provider_id`。保存完整模型状态时，服务端对每一个 Provider 引用执行：

```text
Provider 属于当前身份，或当前身份对 Provider 具有有效分享可见性
```

不把共享 Provider 复制到接收方身份下，也不把 API Key 写入 Endpoint 表。

### 使用统计

现有活动记录已经包含身份、Provider 和 Token 使用信息。统计查询基于以下维度聚合：

- Provider
- 实际使用身份
- 时间周期
- 请求数
- 输入 Token
- 输出 Token
- 总 Token
- 最近使用时间

本期优先复用活动记录查询，不新增配额表或计费账本。若后续数据量证明实时聚合成本过高，再单独设计周期汇总表，并明确其与活动明细的一致性边界。

## 服务端接口

接口命名以现有 `/api/providers` 管理 API 为基础，具体 DTO 先在 `prelay-protocol` 定义，再由服务端和客户端更新 submodule。

建议扩展：

- `GET /api/providers`
  - 返回当前身份自有且可见的共享 Provider。
  - 返回 `owner_identity_id` 和用于展示的 `owner_display_name`。
  - 对共享 Provider 返回只读元数据，`api_key` 为空或不出现在共享响应 DTO 中；不能依赖前端隐藏字段实现保护。
- `PATCH /api/providers/{provider_id}/sharing`
  - 仅 Provider 创建人可调用。
  - 设置 `visibility`，并以完整目标状态提交指定身份列表。
  - `private` 和 `all` 时指定身份列表必须为空。
- `GET /api/providers/{provider_id}/usage`
  - 仅当前可见用户可调用。
  - 返回 Provider 总量和按使用身份拆分的明细。
  - 查询结果不得包含 API Key、设备凭据、Endpoint Token 或其他供应商密钥材料。

创建人对现有 Provider 的编辑和删除接口继续使用原身份归属检查。共享用户调用这些接口时应返回统一的无权错误，不应泄露 Provider 是否存在于其他不可见身份下。

## 关键业务流程

### 创建人分享 Provider

1. 客户端打开 Provider 的分享管理抽屉。
2. 客户端通过 Tauri command 请求当前可授权的已注册身份列表。
3. 创建人选择 `private`、`selected` 或 `all`。
4. Tauri command 调用服务端分享接口。
5. 服务端验证当前身份是 Provider 创建人，校验目标身份存在，并在事务中替换完整授权状态。
6. 成功后返回更新后的 Provider 可见元数据。

### 接收方引用 Provider

1. 客户端刷新供应商列表。
2. 服务端返回当前身份自有 Provider 和仍可见的共享 Provider。
3. Endpoint 表单将共享 Provider 作为只读候选展示。
4. 保存 Endpoint 时服务端重新验证每个 Provider 引用，不能信任客户端之前的列表。
5. Endpoint 保存成功后，协议路由使用原 Provider ID。
6. 协议请求解析 Provider 时再次验证 Provider 与当前 Endpoint 身份的有效关系。

### 撤销分享或删除 Provider

- 撤销指定身份：该身份不能再看到 Provider，也不能保存新的引用；已有 Endpoint 保留原模型记录，但协议解析时跳过无权 Provider，导致该模型没有可用候选。
- 从 `all` 改为 `private` 或 `selected`：所有不满足新范围的身份立即失去访问。
- 删除 Provider：按现有删除流程删除其 Endpoint 模型映射和分享关系；其他身份的相关 Endpoint 映射必须一并清理，或在删除事务中标记为失效，不能留下可继续路由的孤立引用。

## 客户端界面

### 供应商列表

在现有供应商列表增加：

- 创建人
- 共享状态
- 使用统计摘要

共享 Provider 的编辑、删除、Ping 和协议测试操作不可用；创建人保留现有操作。

### 分享与统计抽屉

在 Provider 页面复用现有 Drawer，包含两个区域：

- 分享设置：可见范围、指定用户、保存、撤销分享。
- 使用统计：总请求数、输入 Token、输出 Token、总 Token、趋势和按使用者的明细。

所有能够看到 Provider 的身份都可以打开完整统计。统计界面只展示服务端返回的显示字段，不展示任何凭据。

Endpoint 页面只需要在 Provider 选择项中增加共享来源和创建人信息，不新增平行的“共享 Provider”页面。

所有 Nuxt 页面继续通过 Tauri command 调用服务端；浏览器层不直接请求管理 API，也不持有 device credential。

## 错误与一致性

协议层增加稳定错误码时，先修改 `prelay-protocol`。至少需要区分：

- 分享设置无效。
- 当前身份无权管理分享。
- Provider 对当前身份不可见。
- Endpoint 引用了当前身份无权使用的 Provider。
- Provider 已被删除或分享已撤销，当前路由没有可用候选。

分享状态、指定用户授权和 Provider 删除必须在同一事务中完成。读取路径统一复用可见性查询，避免列表、Endpoint 保存和协议解析使用不同权限规则。

对于正在执行的请求，不主动终止；撤销从后续读取、保存和新请求开始生效。

## 测试与验收

### `prelay-protocol`

- DTO 序列化和反序列化。
- `visibility` 枚举和值校验。
- 共享 Provider 响应不包含可写 API Key 字段。
- 错误码稳定性。

### `prelay-server`

- private Provider 只对创建人可见。
- selected Provider 只对指定身份可见。
- all Provider 对所有已注册身份可见。
- 创建人、指定用户和无权用户的编辑、删除、分享管理权限。
- Endpoint 可以引用可见共享 Provider，不能引用不可见 Provider。
- 撤销分享后已有引用不能继续协议路由。
- 删除 Provider 后分享关系和跨身份引用不会继续生效。
- 统计按 Provider 和实际使用身份聚合。
- 所有可见用户都能读取完整统计。
- 统计响应不包含 API Key、device credential 或 Endpoint Token。
- 自有 Provider 的既有身份隔离行为保持不变。

### `prelay-client`

- 供应商列表显示创建人和共享状态。
- 共享 Provider 只读操作状态正确。
- 分享设置通过 Tauri command 提交。
- Endpoint 表单能选择共享 Provider 并保存。
- 统计抽屉显示总量、趋势和使用者明细。
- 撤销或删除后刷新状态，不保留前端可继续使用的陈旧引用。

## 本期边界

- 不实现配额和超额拦截。
- 不实现请求次数或 Token 限流。
- 不实现跨 `prelay-server` 实例分享。
- 不实现分享链接、邀请审批、组织和角色。
- 不复制 Provider、Endpoint 或 API Key。
- 不把统计扩展为计费、成本结算或供应商账单。

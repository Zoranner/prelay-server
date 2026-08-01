# provider-relay 一致性加固设计

## 目标

本次改动解决当前供应商和接口保存缺少事务边界、接口模型删除越过父资源、测试绕开生产解析链、管理台状态不完整以及构建入口不可重复的问题。

系统仍按单进程 Rust/Axum、SQLite 和 Vue 管理台运行。管理 API 继续保留现有可选 `ADMIN_TOKEN` 行为；当前部署边界限定为受控网络环境。用户、角色、登录和会话体系由后续独立设计处理。

## 约束

- 保留现有 Provider、Interface 和模型表，不引入新实体。
- 保留模型 CRUD API，兼容已有调用方；管理台改用原子保存契约。
- 不引入数据库 migration、外键、队列或版本字段。
- 不改写绑定内网源的 `web/bun.lock`。
- 不启动开发服务，不把自动化测试视为真实供应商、客户端或浏览器验收。

## 保存契约

### Provider

`POST /api/configs` 和 `PUT /api/configs/:id` 的请求体增加 `models` 字段。该字段表示保存后的完整模型名称集合，而不是增量操作。

创建请求必须携带 `models`。更新请求中的 `models` 保持可选：传入时替换完整集合，缺省时保留原集合，以兼容旧调用方。模型名称在服务端去除首尾空白，空名称返回 `400`，重复名称在规范化后返回 `400`。

创建时，Provider 配置和模型集合在同一个 SQLite 事务中写入。更新时，配置字段更新、模型删除和模型新增也在同一个事务中完成。删除 Provider 模型时继续清理引用该上游模型的接口映射；事务失败时配置和模型集合全部回滚。

响应继续使用现有 `ConfigResponse`，返回提交后的完整模型集合。

### Interface

`POST /api/interfaces` 和 `PUT /api/interfaces/:id` 的请求体增加 `models` 字段。每个模型项包含：

- `provider_id`：目标 Provider ID。
- `upstream_model`：Provider 已保存的模型名称。
- `model_name`：客户端使用的模型名称；空白或缺省时使用 `upstream_model`。

创建请求必须携带 `models`。更新请求中的 `models` 保持可选，兼容旧调用方。服务端在事务开始后校验所有 Provider、上游模型和最终 `model_name`。同一 Interface 中规范化后的 `model_name` 必须唯一。

创建时，Interface 配置和全部映射一次提交。更新时，配置字段与完整映射集合一次替换。任何校验或写入失败都回滚，不留下空 Interface，也不丢失旧映射。

响应继续使用现有 `InterfaceResponse`，返回提交后的完整映射集合。

## 资源边界

`DELETE /api/interfaces/:interface_id/models/:model_id` 必须同时匹配 `interface_id` 和 `model_id`。模型属于其他 Interface 时返回 `404`，不得修改其他父资源下的数据。

Provider 模型的删除与完整集合替换沿用现有引用清理规则：删除 `(provider_id, model_name)` 时，同时删除引用该组合的 Interface 映射。由于本轮不增加外键，所有 Provider 模型写入口必须复用同一事务辅助函数，避免不同入口出现不同清理语义。

## 运行时与测试路径

移除 `src/routes/interface_resolver.rs` 中 `cfg(test)` 专用的旧 Provider/alias 回退。测试请求必须提供真实 Interface token，并在测试数据库中创建 Interface、Provider 模型和 Interface 模型映射。

三种协议入口继续统一经过生产中间件和 Interface 解析逻辑。测试 fixture 负责准备数据，不在生产代码中保留测试分支。现有只验证协议转换细节的单元测试可以直接调用纯转换函数；路由测试需要覆盖鉴权、Interface 查询和模型解析。

## 管理台行为

Provider 和 Interface 表单只发送一次创建或更新请求。前端提交完整目标状态，不再调用模型创建/删除 API 编排差集。

页面加载状态明确分为：

- `loading`：首次加载或手动重试正在进行。
- `error`：请求失败，显示可理解的错误信息和重试操作；`401` 明确提示管理凭据无效或缺失。
- `empty`：请求成功但无数据。
- `ready`：请求成功并展示数据。

Provider proxy token 不再写入 `localStorage`。页面初始化时删除旧键 `provider-relay-tokens`，用于清理由当前版本历史代码留下的死数据；管理 API 使用的 `provider-relay-admin-token` 不在本轮调整。

## 构建与仓库入口

`build.rs` 只负责检查并构建已安装的前端依赖。`web/node_modules` 不存在时立即失败，并提示执行：

```text
cd web
bun install --frozen-lockfile
```

Cargo 重新运行条件增加 `web/bun.lock`。`web/package.json` 增加 `test` 和独立 `typecheck` 脚本；`build` 复用 `typecheck` 后执行 Vite 构建。

`.dockerignore` 排除根目录和 `web/` 下的 `.env`、`.env.*`，避免本地环境文件进入构建上下文。README 记录 Bun/Rust 前提、冻结安装、启动参数、`ADMIN_TOKEN` 受控环境边界和完整验证命令。正式设计文档补充 Interface token、完整模型集合与原子保存关系。

## 错误处理

请求数据的空值、重复值和不存在的引用返回 `400`；目标 Provider、Interface 或父子模型不存在返回 `404`；唯一约束或 SQLite 错误继续通过现有 `AppError` 转换处理。事务内产生的任何错误必须在返回前触发回滚。

前端不根据错误后继续刷新来掩盖部分成功，因为保存契约保证全成或全败。保存失败时保留抽屉和用户输入，用户可修正后重试。

## 验证

后端回归测试至少覆盖：

- Provider 创建和更新同时保存完整模型集合。
- Provider 更新失败时配置和旧模型集合保持不变。
- Interface 创建和更新同时保存完整映射集合。
- Interface 引用不存在的 Provider 模型时整笔回滚。
- 跨 Interface 删除模型返回 `404`，目标数据保持不变。
- 路由测试不使用测试专用回退，并通过真实 Interface token 解析模型。

前端测试至少覆盖请求 DTO、单请求保存、历史 token 清理、加载失败与 `401` 状态。仓库验证入口为：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cd web
bun test
bun run format:check
bun run lint
bun run typecheck
bun run build
git diff --check
```

真实供应商、Codex/Claude Code、Docker 镜像、浏览器交互和生产网络仍需在对应环境中单独验收。

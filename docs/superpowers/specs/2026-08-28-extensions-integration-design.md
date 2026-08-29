# 扩展目录合并设计

## 目标

将 `prelay-extensions` 的扩展发现能力合并到 `prelay-server`，让桌面客户端只通过 Prelay 管理 API 发现、查看和安装扩展。独立服务、客户端直连 Gitea、客户端保存扩展源地址均不再保留。

## 边界

- 扩展目录属于 `/api/extensions/*`，使用设备凭据认证；不进入 `/v1`。
- 保持规则、Skill、插件、MCP 四个分类目录接口；不提供聚合目录接口。
- 所有扩展类型都返回受限的固定版本安装文件包；MCP 在下发前校验 `server.json` 共享清单。
- 服务端仅接受配置的 Gitea 地址、组织和只读令牌；客户端不能提供上游 URL、仓库路径或令牌。
- 不创建数据库表，不保存扩展目录快照，不提供独立扩展服务生命周期。

## 协议

`prelay-protocol/src/extensions.rs` 是客户端与服务端的唯一 DTO 来源，定义：

- `ExtensionKind`：`rule`、`skill`、`plugin`、`mcp`。
- `ExtensionVersion`：正式 tag、固定 commit SHA 与发布时间。
- `ExtensionSummary`：仓库名、仓库链接和最新正式版本。
- `ExtensionInstallBundle`：仓库名、类型、固定版本与 Rule/Skill 的 UTF-8 文件列表。

管理路由如下：

| 路由 | 返回 |
| --- | --- |
| `GET /api/extensions/rules` | 规则目录 |
| `GET /api/extensions/skills` | Skill 目录 |
| `GET /api/extensions/plugins` | 插件目录 |
| `GET /api/extensions/mcp` | MCP 目录 |
| `GET /api/extensions/{name}/versions` | 正式版本列表 |
| `GET /api/extensions/{name}/versions/{tag}/readme` | 固定版本 README |
| `GET /api/extensions/{name}/versions/{tag}/install` | Rule/Skill 固定版本安装文件包 |

新增稳定错误码：目录尚无成功快照时的 `extension_catalog_unavailable`、仓库不存在的 `extension_not_found`、版本不存在的 `extension_version_not_found`。Gitea 的 URL、状态码和响应内容不得进入错误响应或日志字段。

## 服务端结构

```text
src/
  extensions/
    mod.rs
    config.rs
    catalog.rs
    gitea.rs
    package.rs
  routes/
    api/
      extensions.rs
```

`config.rs` 只读取和校验环境变量。`gitea.rs` 封装 Gitea HTTP DTO 与请求。`package.rs` 负责仓库名、tag、commit SHA、文件路径与扩展类型判定。`catalog.rs` 持有内存快照、单次刷新协调和按固定版本读取 README 或安装文件。路由只将 HTTP 请求映射为协议 DTO。

`AppState` 持有一个 `ExtensionCatalog`，与现有 `ClientUpdateCache` 一样由 `main.rs` 使用共享 `reqwest::Client` 初始化。服务启动时异步预热目录；Gitea 不可达不阻止服务启动。有效快照直接返回，过期快照先返回并触发一次后台刷新，首次尚无快照时返回稳定的目录不可用错误。

## 配置

```text
EXTENSIONS_GITEA_URL
EXTENSIONS_GITEA_ORGANIZATION
EXTENSIONS_GITEA_READ_TOKEN
EXTENSIONS_CATALOG_CACHE_TTL_SECONDS
```

这四项属于扩展目录配置域。URL 仅允许绝对 HTTP 或 HTTPS 地址，组织名、仓库名、tag、commit SHA 与路径均在服务端验证。令牌为空时不发送认证头，且不写入响应、客户端持久化状态或日志。

## 客户端结构

Tauri 扩展命令经 `authenticated_api` 请求 `/api/extensions/*`。本机模块仅负责将服务端返回的 Rule 合并到既有规则文件，或将服务端返回的 Skill 文件原子写入既有目标目录。

Nuxt 侧按 `ExtensionKind` 独立保存目录、加载状态与请求中的 Promise。首次进入扩展库仅加载当前分类，切换分类才请求对应接口；README、版本和安装包均按用户操作延迟读取。切换 Prelay 服务地址时清空四类目录和详情缓存。

## 迁移与退役

先提交协议，再提交服务端模块和 API 路由，再替换客户端直连 Gitea 的实现与页面状态。部署环境完成变量迁移，并由客户端验证目录、README 和 Rule/Skill 安装后，才删除独立服务的 Docker、Compose 与仓库。迁移期间不保留旧环境变量别名或两条发现链路。

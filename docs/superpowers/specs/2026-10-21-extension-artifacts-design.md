# 扩展包制品化设计

## 背景

当前扩展目录只有规则、Skill、MCP 三类，一个仓库只能属于其中一类。类型由文件树推断：`server.json` 判为 MCP，`skills/*/SKILL.md` 判为 Skill，`AGENTS.md` 判为规则。安装时只收集该类型对应的路径，其余文件不报错、不提示，直接丢弃。

这带来两个实际后果：

- 需要同时携带 Skill 和 Hook 的包无法分发。superpowers 的 14 个 Skill 与 `SessionStart` 注入属于同一个功能单元，现在只能发 Skill 部分，Hook 无处安放。
- 包内多放一个文件不会让安装失败，只会让内容悄悄消失。用户看到“安装成功”，实际只装了一半。

2026-08-28 的设计曾包含四类目录（规则、Skill、插件、MCP），2026-09-01 的 `ffff3fb` 移除了插件目录，原因是当时只识别插件清单，没有对应的安装路径。本次不恢复那套插件识别，而是把“插件”定义为多制品扩展包的展示分类，安装能力由制品决定。

## 目标

- 扩展库只分两类：规则、插件。
- 插件是至少包含一个制品的扩展包，制品类型为 MCP、Skill、Hook，可以组合。
- 已安装视图分五类：规则、插件、MCP、Skill、Hook。Prelay 记录的包聚合为插件，非 Prelay 管理的条目按实际类型独立显示。
- 现有已发布的单制品包不需要修改任何文件，分类与安装结果保持不变。

## 非目标

- 不恢复旧的插件目录识别，不把 `.codex-plugin/plugin.json` 当作可管理扩展。
- 不安装 Codex 原生插件，不管理插件运行时生命周期。
- 不做依赖解析、版本范围、远程安装脚本。
- 不做通用插件市场。

## 术语

**规则包**：内容为 `AGENTS.md`，安装语义是合并到宿主的全局规则文件。

**插件**：Prelay 的可分发扩展包，包含一个或多个制品。插件不是 Codex 原生插件，两者只在名称上相近。

**制品**：`mcp`、`skill`、`hook` 之一，对应一种安装目标和一组文件。规则不进入插件，始终独立成包。

**安装记录**：客户端本地保存的“包到制品”的映射，用于聚合展示、更新和卸载。

## 协议

`prelay-protocol/src/extensions/mod.rs` 改造前只定义 `Rule`、`Skill`、`Mcp`。调整为：

```rust
pub enum ExtensionKind {
    Rule,
    Skill,
    Mcp,
    Hook,
    Unknown,
}

pub struct ExtensionArtifact {
    pub kind: ExtensionKind,
    pub files: Vec<ExtensionFile>,
}

pub struct ExtensionInstallBundle {
    pub name: String,
    pub version: ExtensionVersion,
    pub artifacts: Vec<ExtensionArtifact>,
    pub skipped: Vec<String>,
}
```

协议目录条目 `ExtensionSummary` 增加 `catalog_kind` 和制品摘要，目录列表不再依赖单一 `kind`：

```rust
pub enum CatalogKind {
    Rule,
    Plugin,
}

pub struct ExtensionArtifactSummary {
    pub kind: ExtensionKind,
    pub file_count: usize,
}

pub struct ExtensionSummary {
    pub name: String,
    pub repository: String,
    pub latest: ExtensionVersion,
    pub catalog_kind: Option<CatalogKind>,
    pub artifacts: Vec<ExtensionArtifactSummary>,
}
```

`CatalogKind` 和 `ExtensionArtifactSummary` 属于协议 DTO。`ExtensionPackage` 是客户端的展示模型，位于 `prelay-client/src-tauri/src/extensions/model.rs`，在本地扩展 `catalog_kind`、`artifacts`、`install_action` 和 `installed_clients`，不进入协议 crate。

协议把无法识别的 `kind` 解析为 `Unknown`，避免整个目录响应反序列化失败。服务端解析 `.prelay.json` 遇到未知类型时返回稳定错误 `extension_content_invalid`，不因单个包让整个目录请求不可用；客户端遇到未知制品时提示升级 Prelay，并保留其余制品可用。

## 服务端

### 目录分类

`classify_paths` 从“返回一个类型”改为“返回制品列表”。规则如下：

- 只有 `AGENTS.md`，没有其他制品，判为规则包。
- 有 `server.json`，产出 MCP 制品。
- 有 `skills/*/SKILL.md`，产出 Skill 制品。
- 有 `hooks/hooks.json`，产出 Hook 制品。
- 同时存在多个制品，判为插件。
- 存在制品时忽略根目录 `AGENTS.md`，避免把开发说明写进用户的全局规则。插件不包含规则；即使 `.prelay.json` 声明 `rule`，也按无效内容拒绝，`AGENTS.md` 只能独立成规则包。

仓库根可选的 `.prelay.json` 用于消除推断歧义：

```json
{
  "schema": 2,
  "artifacts": [
    { "kind": "skill", "include": ["skills/"] },
    { "kind": "hook", "path": "hooks/hooks.json" }
  ]
}
```

有清单时以清单为准；声明了但目录不存在、路径越界或制品为空时返回稳定错误，不做静默降级。

`.prelay.json` 只允许声明 `mcp`、`skill`、`hook` 三类制品；`rule` 和未知类型返回 `extension_content_invalid`。

### 安装包

`install_bundle` 按制品收集文件，不再按单一类型过滤整个仓库。未匹配任何制品的文件进入 `skipped` 列表并随安装结果返回。规则包与插件包的文件都必须通过现有的路径校验，不接受绝对路径、反斜杠和 `..`。

单制品 Rule、Skill、MCP 安装包在 `artifacts` 之外继续输出旧客户端的 `kind/files` 字段；多制品和 Hook 包不输出旧字段。新客户端只以 `artifacts` 为准，旧服务端的 `kind/files` 响应由协议反序列化兼容为单制品 `artifacts`。

Hook 制品只允许 `hooks/*.json`（2026-10-24 修订为 `hooks/**`，见文末）。命令和脚本内容由 Codex 在信任审核时展示，服务端不做执行，也不校验命令是否可运行。

### 目录接口

- `GET /api/extensions/rules` 返回规则包。
- `GET /api/extensions/plugins` 返回插件包。
- `GET /api/extensions/skills` 和 `/api/extensions/mcp` 保留一个版本，按制品类型过滤插件目录，标记为过渡接口。

新客户端只使用前两个接口。过渡接口的移除需要单独确认，不在本次变更内。

## 客户端

### 安装分派

`install_extension` 从匹配单个 `bundle.kind` 改为遍历 `bundle.artifacts`：

- `skill` 复用 `skills::install_skill_files`，写入现有 Skill 根目录。
- `mcp` 复用 `mcp::install_mcp`，合并 `config.toml`。
- `hook` 新增 `hooks::install_hooks`，合并 `~/.codex/hooks.json`。
- `rule` 复用 `rules::install_rule`，但规则包不会与插件同时出现。

### Hook 安装

Hook 的目标文件是 `~/.codex/hooks.json`，结构为事件到 matcher 组到 handler 的映射。安装遵循合并语义：

- 文件不存在时创建；内容不是合法 JSON 时直接报错，不覆盖。
- 每个 handler 用 `event + matcher + type + command` 的稳定哈希做身份。
- 相同哈希视为同一安装，替换而不是追加；不同哈希追加，不修改用户手写条目。
- 卸载时只删除安装记录中的哈希，保留其他条目。

### 安装记录

现有 `~/.agents/.prelay/skill.json`、`~/.codex/.prelay/mcp.json`、`~/.codex/.prelay/rule.json` 分别记录不同制品，聚合视图需要读取四份文件。改为统一记录：

```json
{
  "superpowers": {
    "version": "v6.3.1",
    "commitSha": "...",
    "targets": {
      "codexCli": {
        "skills": ["using-superpowers"],
        "hooks": ["SessionStart:..."]
      },
      "openCode": {
        "skills": ["using-superpowers"]
      }
    }
  }
}
```

旧文件保留只读迁移：首次写入时读取并合并到统一记录，之后以统一记录为准。迁移失败时不删除旧文件。

### 安装顺序

1. 拉取安装包并校验版本与 commit 一致。
2. 预检所有制品：路径合法、JSON 可解析、目标可写、Hook 事件名合法。
3. 按制品写入，每个文件原子替换。
4. 写入统一安装记录。

预检失败时不写任何文件。写入阶段失败时恢复已修改的 `hooks.json`、`config.toml` 和规则文件；Skill 目录按安装前备份恢复。

### 信任提示

Codex 对非托管 Hook 要求用户审核并信任，未信任的 Hook 不会运行。安装结果必须包含“需要审核”的标记，客户端在安装完成后停留展示，而不是直接关闭。提示内容至少包含事件、matcher 和命令，不能只写“包含 Hook”。

## 前端

扩展库从三个分类改为两个：规则、插件。插件列表用卡片标签显示 `MCP`、`Skill`、`Hook`，标签只用于说明包含内容，不承担导航。

安装抽屉按制品展开：

- Skill：目标目录和技能数量。
- MCP：服务名和 transport。
- Hook：事件、matcher、命令全文，可折叠。

客户端选择按制品处理。Hook 只支持 Codex，Skill 和 MCP 支持 Codex 与 OpenCode，界面需要显示“OpenCode 跳过 Hook”，不能用整包交集直接禁用。

已安装视图保留五类。Prelay 记录的包聚合为一条插件，展开显示成员；非 Prelay 管理的条目按实际类型独立显示。卸载插件清理全部成员，卸载独立条目只清理该条目。

## 兼容

- 现有包都没有 `.prelay.json`，按推断分别归为规则包或单制品插件包，安装结果不变。
- 旧客户端连接新服务端时，`/extensions/skills` 和 `/extensions/mcp` 过渡接口继续可用；单制品包继续保留旧安装字段。
- 新客户端连接旧服务端时，`/extensions/plugins` 不存在则回退 `/extensions/skills` 与 `/extensions/mcp`，把缺少 `catalog_kind` 和 `artifacts` 的响应按单制品包处理，并禁用 Hook 安装。
- 统一安装记录上线后，旧记录文件不再写入，但保留读取直到确认无回退需求。

## 分阶段

1. `prelay-protocol`：增加 `Hook`、`ExtensionArtifact`、`CatalogKind`，调整安装包结构。
2. `prelay-server`：分类改为规则/插件，按制品打包，增加 `skipped` 和过渡接口。
3. `prelay-client` 原生层：遍历制品安装，新增 Hook 合并与统一安装记录。
4. `prelay-client` 前端：两个分类、插件标签、安装抽屉、已安装聚合。
5. `superpowers`：增加 `hooks/hooks.json` 与 `hooks/scripts/`，不使用 `.prelay.json`（见文末修订），新增版本 tag，不移动已发布 tag。

协议和服务端可以独立验证，不依赖客户端 UI。客户端原生层完成后才能验证真实 Hook 写入，前端可以并行开发。

## 验证

`prelay-protocol` 和 `prelay-server` 按仓库规则执行：

```text
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
```

服务端测试需要 `TEST_POSTGRES_URL` 指向专用测试库。至少覆盖：

- 单制品包仍按原类型安装。
- 多制品包产出的 artifacts、文件和 `skipped` 正确。
- 清单缺失路径、越界路径、空制品被拒绝。
- 过渡接口返回与原分类一致的结果。

`prelay-client` 前端执行 `bun test`、`bun run typecheck`、`bun run generate`，原生层执行 `cargo fmt`、`cargo clippy`、`cargo test`。至少覆盖：

- 插件包在两个分类下不重复安装。
- Hook 合并保留用户条目，卸载只删除自己的条目。
- 预检失败不产生部分写入。

真实 Codex 的 Hook 信任流程、`SessionStart` 实际注入和 Windows 命令执行属于人工联调，不能用单元测试替代。

## 风险

**分类漂移。** 单制品包增加制品后会从 Skill 或 MCP 变为插件，用户在原位置找不到。插件卡片必须显示制品标签，README 变更说明需要提示新增制品。

**Hook 合并冲突。** 用户手改 `hooks.json` 或 Codex 自身写入条目时，哈希匹配可能失效。处理原则是不覆盖未知条目，安装记录与文件不一致时报告冲突，由用户决定是否覆盖。

**信任流程阻断。** 安装成功但未信任时 Hook 不生效。安装结果必须区分“已写入”和“已生效”，否则会把未生效说成完成。

**Windows 命令差异。** Hook 命令在 Windows 上需要单独的命令形式，包内提供 `command` 与 `commandWindows` 两份，不能只验证 Unix 路径。

## 已确认

- 统一安装记录在本次一次迁移：统一记录是唯一写入源，首次写入时合并旧文件，旧文件只读保留；迁移失败不删除旧文件。
- OpenCode 没有等价的 Hook 机制，插件在 OpenCode 下安装 Skill 和 MCP，跳过 Hook，界面明确标注。
- `/extensions/skills` 和 `/extensions/mcp` 至少保留到新客户端版本发布后，移除需要单独确认。
- `superpowers` 新增版本 tag，不移动已发布 tag；发布动作单独确认。

## 实现修订（2026-10-24）

以下内容替代前文的 Hook 单文件与 `.prelay.json` 约定：

- Hook 制品收录 `hooks/**`。`hooks/hooks.json` 是唯一定义文件，可包含多个事件、matcher 组和 handler；`hooks/` 下的其他文件是支持文件，不参与定义解析。
- 服务端只对 `hooks/hooks.json` 做 UTF-8 与 JSON 校验，支持文件按原始字节打包。
- 客户端把整个 `hooks/` 复制到 `~/.prelay/packages/<package>/<commit-sha>/hooks/`，命令中的 `${PRELAY_HOOK_ROOT}` 在写入 `~/.codex/hooks.json` 前展开；升级到新 commit 时旧目录被清理，Hook 信任记录随命令变化重新审核。
- 包不再使用 `.prelay.json`。`superpowers` 的 Skill 与 Hook 由标准目录推断；组织级规则以 `agents/README.md` 为准。

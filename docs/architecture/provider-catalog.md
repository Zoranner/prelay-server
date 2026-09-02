# 供应商与模型目录

## 决策

供应商目录和模型目录由服务端部署配置文件统一维护。它是供应商调用规则、模型白名单和模型思考档位的唯一来源。

目录不是数据库资源，不按设备或身份区分，也不提供桌面端编辑能力。运维修改配置文件后重启服务；桌面客户端在打开供应商或接入点配置时，从服务端读取当前加载的目录。

本设计移除以下旧概念：

- 前端内置的供应商目录事实。
- 上游模型发现和“获取模型”操作。
- 自定义供应商目录项、自定义模型名和模型别名。
- `provider_type`、`capabilities_json`、协议 URL 特例和按类型推导的默认规则。
- 接入点模型映射中的 `upstream_model`。

保留的是用户创建的供应商接入：同一个目录供应商可以保存多个名称、API Key 或 URL 不同的接入。

## 配置文件

配置目录包含两份文件：`/app/config/models.toml` 与 `/app/config/providers.toml`。本地开发可通过部署环境提供对应文件。两个文件都不包含 API Key、Endpoint Token、设备凭据或数据库信息。

`models.toml`：

```toml
[[models]]
id = "gpt-5.6-luna"
display_name = "GPT-5.6 Luna"
model_type = "text"
reasoning_efforts = ["low", "medium", "high"]
default_reasoning_effort = "medium"

[[models]]
id = "gpt-5.6-terra"
display_name = "GPT-5.6 Terra"
model_type = "text"
reasoning_efforts = ["low", "medium", "high"]
default_reasoning_effort = "medium"

[[models]]
id = "gpt-5.6-sol"
display_name = "GPT-5.6 Sol"
model_type = "text"
reasoning_efforts = ["low", "medium", "high"]
default_reasoning_effort = "medium"

[[models]]
id = "deepseek-v4-flash"
display_name = "DeepSeek V4 Flash"
model_type = "text"
reasoning_efforts = ["low", "medium", "high"]
default_reasoning_effort = "medium"

[[models]]
id = "deepseek-v4-flash-vision-exp"
display_name = "DeepSeek V4 Flash Vision"
model_type = "text"
reasoning_efforts = ["low", "medium", "high"]
default_reasoning_effort = "medium"

[[models]]
id = "deepseek-v4-pro"
display_name = "DeepSeek V4 Pro"
model_type = "text"
reasoning_efforts = ["low", "medium", "high"]
default_reasoning_effort = "medium"

[[models]]
id = "gpt-image-1"
display_name = "GPT Image 1"
model_type = "image"
reasoning_efforts = []

[[models]]
id = "gpt-image-1.5"
display_name = "GPT Image 1.5"
model_type = "image"
reasoning_efforts = []

[[models]]
id = "gpt-image-2"
display_name = "GPT Image 2"
model_type = "image"
reasoning_efforts = []
```

`providers.toml`：

```toml
[[providers]]
id = "gotoken"
name = "GoToken 套餐"
auth_scheme = "bearer"
base_url = "https://gotoken.cc"
protocols = ["chat_completions", "responses", "anthropic_messages", "images_generations"]
models = [
  "gpt-5.6-luna",
  "gpt-5.6-terra",
  "gpt-5.6-sol",
  "gpt-image-1",
  "gpt-image-1.5",
  "gpt-image-2",
]

[providers.protocol_base_urls]
chat_completions = "https://gotoken.cc/v1"
anthropic_messages = "https://gotoken.cc/v1"
images_generations = "https://gotoken.cc/v1"

[[providers]]
id = "deepseek"
name = "DeepSeek 开放平台"
auth_scheme = "bearer"
base_url = "https://api.deepseek.com/v1"
protocols = ["chat_completions", "responses", "anthropic_messages"]
models = [
  "deepseek-v4-flash",
  "deepseek-v4-flash-vision-exp",
  "deepseek-v4-pro",
]

[providers.protocol_base_urls]
anthropic_messages = "https://api.deepseek.com/anthropic"

```

上述条目采用当前已有的 GoToken、DeepSeek 配置和当前已保存模型名。其他现有供应商与其白名单模型按相同格式补入目录。

### 模型目录

每个 `models` 条目定义一个系统允许配置的模型：

- `id` 是稳定模型标识，也是接入点和上游请求使用的精确模型名。
- `display_name` 仅用于界面显示，不参与路由或上游请求。
- `model_type` 是模型类型，当前取值为 `text` 或 `image`。
- `reasoning_efforts` 是该模型唯一的思考档位事实。空数组表示该模型没有可选思考档位。
- `default_reasoning_effort` 只在 `reasoning_efforts` 非空时出现，且必须属于该数组。

档位只能使用当前 Codex 目录格式可识别的值：`none`、`minimal`、`low`、`medium`、`high`、`xhigh`。不接受 `max` 或模型目录外的任意字符串。

同一模型只在全局目录中定义一次。供应商目录只引用模型 ID，不复制模型能力，因此多个供应商提供同名模型时不会出现思考档位不一致。

文生图模型的 `model_type` 为 `image`，并且没有思考档位。模型类型不保存、声明或选择上游协议；协议只由供应商目录定义。

### 供应商目录

每个 `providers` 条目定义一种完整的上游调用规则：

- `id` 是稳定供应商标识，已保存供应商接入引用它。
- `name` 是供应商显示名称。
- `auth_scheme` 是该供应商所有协议入口使用的认证方式，当前取值为 `bearer` 或 `anthropic`。
- `base_url` 是该供应商的默认 URL。
- `protocols` 是供应商支持的上游协议，当前取值为 `responses`、`chat_completions`、`anthropic_messages`、`images_generations`。
- `protocol_base_urls` 是协议级 URL 覆盖；键必须属于 `protocols`。未定义某协议 URL 时，该协议使用 `base_url`。
- `models` 是该供应商允许提供的模型 ID 列表；每个值必须存在于全局模型目录。

供应商目录不再声明 `provider_type`。认证方式、协议 URL 和模型集合已经完整表达服务端调用上游所需的事实，不需要再由代码按类型进行二次推导。

供应商目录也不声明宽泛的 `capabilities`。现有该字段主要用于 `/v1/models` 的展示元数据，不能可靠表示未知上游服务的真实模型能力，也不参与上游请求决策。本设计不以供应商级开关对外承诺工具调用、结构化输出等能力；若未来确实需要对外声明和约束这些能力，应单独以模型能力或“供应商与模型”的组合能力设计。

### 协议选择顺序

`protocols` 只声明供应商实际支持的协议集合，不承担请求选择优先级。服务端按当前固定规则，针对每个下游入口从该集合中选择第一个兼容协议：

| 下游入口 | 固定上游选择顺序 |
| --- | --- |
| Responses | `responses` → `chat_completions` → `anthropic_messages` |
| Chat Completions | `chat_completions` |
| Anthropic Messages | `anthropic_messages` → `chat_completions` → `responses` |
| Images Generations | `images_generations` |

配置文件中的 `protocols` 数组必须按统一顺序书写：

```text
chat_completions → responses → anthropic_messages → images_generations
```

每个供应商只保留自己支持的协议，但不得改变剩余协议的相对顺序。例如支持 Chat Completions 和 Anthropic Messages 的供应商必须写为 `["chat_completions", "anthropic_messages"]`。协议 URL 覆盖也按同一顺序排列。

服务端实现仍按下游入口的固定选择顺序和供应商支持集合选择上游协议；配置数组的固定顺序用于保持配置、API 输出和表单展示一致。

## 已保存的供应商

用户保存的供应商实例只包含：

```text
名称 + provider_id + base_url 覆盖 + protocol_base_urls 覆盖 + 加密 API Key
```

名称可自由填写，用于区分不同账号或不同部署地址。`provider_id` 必须引用当前服务端供应商目录。数据库内部的供应商接入主键与该目录 ID 分离。

用户可覆盖供应商目录的 `base_url` 与已声明的 `protocol_base_urls`。协议 URL 覆盖只能替换模板已有协议中的键，不能新增协议、清空 URL、改变认证方式、添加模型或修改思考档位。

有效上游 URL 按以下顺序确定：

```text
供应商接入的 protocol_base_urls[协议]
-> 供应商目录的 protocol_base_urls[协议]
-> 供应商接入的 base_url
-> 供应商目录的 base_url
```

这不是自定义供应商目录。未知供应商只有在兼容既有供应商目录的认证和协议规则、并实际提供白名单模型时，才能通过选择目录供应商并覆盖 URL 接入。

## 接入点模型与候选供应商

接入点模型先从全局模型目录正常新增。模型 ID 一经选定，即是该模型组的唯一名称。

为已有模型增加供应商时：

- 模型下拉仍保留，但只显示当前模型组的模型 ID。
- 供应商下拉只显示其目录项 `models` 包含该模型 ID 的已保存供应商。
- 服务端再次执行同样校验；桌面端过滤不能代替服务端约束。

接入点候选只保存：

```text
model_id + provider_instance_id + candidate_order
```

不保存公开名到上游名的映射。候选切换时，所有供应商都接收同一个模型 ID，因而不会在故障转移中跨模型。

## 调用与目录生成

服务端处理请求时，先从已保存供应商接入取得 `provider_id` 和 URL 覆盖，再解析供应商目录得到有效认证方式、上游协议和协议 URL。服务端不再使用 `provider_type`、URL 修正规则或模型发现回退路径。

`GET /v1/models` 仍只返回当前接入点已配置的模型，不暴露全局目录中尚未配置到该接入点的模型。

桌面客户端保存 Codex 设置时，从服务端返回的接入点模型定义读取 `reasoning_efforts` 和默认档位，直接生成本机 `.codex/models.json`。客户端不再维护 `deepseek_models.json` 或供应商目录副本。

## 管理接口与客户端边界

服务端提供只读目录接口，例如 `GET /api/provider-catalog`。响应合并模型目录、供应商目录及其模型引用，但绝不包含任何已保存的 API Key。

供应商创建和更新请求只接受名称、供应商 ID、允许范围内的 URL 覆盖和 API Key。接入点创建和更新请求只接受白名单模型与有效供应商候选。

桌面客户端不再调用模型发现接口，也不提供自定义供应商、自由模型输入、能力覆盖、认证方式覆盖或协议覆盖入口。它每次进入供应商或接入点配置流程时读取服务端目录，以当前服务端配置填充和过滤表单。

## 启动校验与配置变更

服务在开始监听前必须校验：

- 模型 ID 和供应商 ID 均唯一。
- 供应商引用的模型存在。
- 模型类型属于允许枚举；`image` 模型没有思考档位。
- `protocols` 非空、不重复，且每项属于允许的协议值。
- 默认 `base_url` 非空；协议 URL 覆盖只使用已声明的协议键。
- `auth_scheme` 有效，且用户无法覆盖。
- 思考档位属于允许枚举。
- 非空思考档位集合具有有效默认值；空集合没有默认值。
- 数据库中已保存的供应商目录引用仍存在。
- 数据库中已保存的接入点模型仍在目录中，且其候选供应商允许该模型。

任何一项失败都拒绝启动并报告具体引用关系，不以默认值、未知供应商或模型名猜测继续运行。

供应商 ID 与模型 ID 是持久化引用，不得通过直接改名或删除来“更新”现网目录。需要下线供应商或模型时，应先移除或迁移其保存供应商接入和接入点引用，再修改配置文件。

第一版不监听文件变化，也不进行热重载。修改配置文件后重启服务，使加载结果和所有引用校验形成一个确定的启动事务。

## 迁移与发布边界

当前服务对既有数据库不提供通用 schema 升级。采用本设计不能声称现有供应商、模型和接入点会被自动迁移。

部署新结构时有两种明确路径：

- 使用新的空数据库，按服务端目录重新创建供应商接入和接入点。
- 在另行授权的迁移任务中，编写一次性迁移工具；它只迁移能精确对应供应商 ID 和白名单模型的记录，无法对应的自定义供应商或模型映射必须报出并由运维人工处理。

协议 DTO、服务端存储和路由、桌面端表单与 Codex 设置生成属于三个独立仓库。实施时先变更 `prelay-protocol` 的管理 DTO，再更新服务端和客户端调用方；不得在任一父仓复制协议类型。

不为旧桌面管理客户端保留兼容层。服务端切换到目录驱动的管理 DTO 后，旧客户端不能继续新增或编辑供应商、接入点；这是有意的管理面版本边界。

智能体调用不依赖桌面管理客户端。迁移必须保留既有 Endpoint Token、接入点模型名和候选供应商关系，且不改变 `/v1/models`、`/v1/responses`、`/v1/chat/completions`、`/v1/messages` 的调用契约。满足这些条件时，已配置智能体继续使用原有连接，不受旧管理客户端不兼容影响。

## 验证目标

实施后至少验证以下事实：

- 两份目录文件可解析，非法引用和非法档位会阻止服务启动。
- 服务端拒绝目录外模型、目录外协议 URL 覆盖和不支持当前模型的候选供应商。
- 同一接入点模型增加多个候选时，所有候选供应商均声明该精确模型名。
- 文生图模型只由图像生成调用链解析，文本模型不进入该调用链。
- 迁移后使用既有 Endpoint Token 调用四个 `/v1/*` 入口，确认智能体调用不依赖旧管理客户端。
- 上游请求使用供应商目录认证方式及用户允许的 URL 覆盖。
- 客户端表单不再出现模型发现、自定义模型或自定义供应商目录入口。
- Codex 目录只包含接入点实际配置的模型和该模型声明的有效思考档位。
- 故障切换仍只在同一模型组的候选供应商之间发生。

## 排除项

本设计不引入供应商目录数据库 CRUD、目录热重载、模型自动发现、按请求探测模型能力、模型别名、前端本地目录缓存、配额计费或供应商健康评分。

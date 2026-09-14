# 供应商与模型目录

## 模型类别

语言模型和图像生成模型是不同的领域对象，不使用 `model_type` 在同一个模型结构内区分。

- 语言模型用于 `/v1/responses`、`/v1/chat/completions`、`/v1/messages` 和 Codex 模型目录生成，拥有上下文窗口、思考档位与工具元数据。
- 图像生成模型只用于 `/v1/images/generations`，拥有输入输出模态、尺寸、质量、背景、输出格式与编辑能力。

服务启动时从当前工作目录下的 `config/catalog` 读取目录。容器工作目录为 `/app`，因此使用 `/app/config/catalog`；本地调试时使用项目根目录的 `config/catalog`。目录无效时服务拒绝启动。

## 配置目录

配置目录包含三份文件：

```text
models/language.toml
models/image-generation.toml
providers.toml
```

它们不包含 API Key、Endpoint Token、设备凭据或数据库内容。

`models/instructions/` 目录按语言模型 id 存放 `<id>.md` 基础指令模板，并以 `_default.md` 作为没有专属模板时的默认指令；加载时只按文件名精确查找，不扫描目录。模板文件内容按原文读取，不做裁剪。

### 语言模型

`models/language.toml` 的字段顺序固定如下：

```toml
id = "模型标识"
display_name = "模型展示名称"
description = "模型描述"
reasoning_efforts = ["low", "high"]
default_reasoning_effort = "high"
context_window = 1048576
max_context_window = 1048576
effective_context_window_percent = 95
input_modalities = ["text", "image"]
supports_parallel_tool_calls = true
supports_reasoning_summaries = true
supports_image_detail_original = true
support_verbosity = true
default_verbosity = "low"
apply_patch_tool_type = "freeform"
web_search_tool_type = "text"
truncation_policy = { mode = "tokens", limit = 10000 }
reasoning_summary_format = "experimental"
default_reasoning_summary = "none"
shell_type = "shell_command"
visibility = "list"
supported_in_api = true
priority = 0
base_instructions = "基础指令"
experimental_supported_tools = []
minimal_client_version = "0.144.0"
```

官方资料未明确的字段以英文键名注释保留；缺省表示能力未知，不由客户端推断为具体能力。`reasoning_efforts` 只能使用 `none`、`minimal`、`low`、`medium`、`high`、`xhigh` 或 `max`；配置默认思考强度时必须同时给出非空档位列表，且 `default_reasoning_effort` 必须包含在该列表中。默认思考强度由服务端目录维护，客户端用户设置可以覆盖它。目录约定：可选档位超过三档时默认使用 `high`，三档或更少时使用列表中的最高档。

目录条目默认不配置 `base_instructions`：缺省或为空白时，加载器依次使用 `models/instructions/<id>.md` 与 `models/instructions/_default.md`；条目显式填写的值优先于这两份模板，均为空时保持空。

### 图像生成模型

`models/image-generation.toml` 只保留图像生成能力，字段顺序固定如下：

```toml
id = "模型标识"
display_name = "模型展示名称"
description = "模型描述"
input_modalities = ["text", "image"]
output_modalities = ["image"]
sizes = ["1024x1024"]
quality_options = ["standard", "high"]
background_options = ["transparent", "opaque"]
output_formats = ["png", "jpeg"]
supports_editing = true
supports_mask = true
supports_reference_images = true
visibility = "list"
supported_in_api = true
priority = 0
```

图像生成目录不接受上下文、思考、Shell、补丁、工具或 Codex 字段。`output_modalities` 当前只接受 `image`；官方资料未明确的能力继续以字段名注释保留。

### 供应商

每个 `providers.toml` 条目按两类模型分别引用：

```toml
[[providers]]
id = "gotoken"
name = "GoToken 套餐"
auth_scheme = "bearer"
base_url = "https://gotoken.cc"
protocols = ["chat_completions", "responses", "anthropic_messages", "images_generations"]
language_models = ["gpt-5.6-sol"]
image_generation_models = ["gpt-image-1"]

[providers.protocol_base_urls]
chat_completions = "https://gotoken.cc/v1"
images_generations = "https://gotoken.cc/v1"
```

`protocols` 必须按 `chat_completions`、`responses`、`anthropic_messages`、`images_generations` 的相对顺序排列。图像生成模型引用存在时，供应商必须声明 `images_generations`。两类引用分别只能指向对应目录中的模型，不能重复或交叉引用。

同一模型在不同平台的上游名称不同时，供应商条目补充该供应商使用的上游模型名，其余情况不需要声明：

```toml
[[providers]]
id = "tokenharbor"
language_models = ["k3"]

[providers.upstream_model_names]
k3 = "kimi-k3"
```

映射的键必须是该供应商已引用的模型 id，值为该平台实际接受的上游模型名；引用未声明的模型或空值都会让服务拒绝启动。模型身份、能力、显示名和对外模型名始终以目录模型 id 为准，接入点保存的上游名只用于发往上游的请求，因此同一个模型在不同供应商处可以各自解析到不同的上游名。

供应商支持的上游协议只由目录决定：服务端在运行期按 `providers.toml` 的 `protocols` 判定可用协议，管理 API 不接受客户端提交的协议集合。各协议地址可以在供应商记录里按协议覆盖（客户端表单默认填入目录里的地址，留空表示使用该供应商记录的 base_url），因此员工要把一个供应商指向自己的网关时，改地址即可，不需要新增目录条目。

## 接口边界

认证后的目录接口分别返回供应商或单类模型数组：

```text
GET /api/catalog/providers
GET /api/catalog/providers/{provider_id}
GET /api/catalog/models/language
GET /api/catalog/models/language/{model_id}
GET /api/catalog/models/image-generation
GET /api/catalog/models/image-generation/{model_id}
```

供应商目录对象使用 `language_models` 与 `image_generation_models` 引用两类模型。语言模型响应保留完整 Codex 目录元数据；图像生成模型响应只返回图像生成字段。

接入点模型列表也按调用类别拆分：

| 路径 | 内容 |
| --- | --- |
| `GET /v1/models` | 当前接入点已配置的语言模型，返回 OpenAI 标准模型对象字段，`id` 为目录模型 id |
| `POST /v1/images/generations` | 仅解析图像生成模型候选 |

`/v1/responses`、`/v1/chat/completions` 与 `/v1/messages` 仅解析语言模型候选。服务端按候选的目录模型 id 在启动时加载的目录中判定类别，发往上游时再替换为该供应商的上游模型名，因此同一接入点的模型不会同时进入语言与图像生成链路。

## 目录生成

Codex 的 `models.json` 只应消费 `language_models`。图像生成模型不生成 Codex 模型目录项，也不携带语言模型的空字段占位。

目录不热重载。修改任一 TOML 文件后需要重启服务，使模型分类、供应商引用与接口输出在同一个启动配置中生效。

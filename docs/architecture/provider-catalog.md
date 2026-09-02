# 供应商与模型目录

## 模型类别

语言模型和图像生成模型是不同的领域对象，不使用 `model_type` 在同一个模型结构内区分。

- 语言模型用于 `/v1/responses`、`/v1/chat/completions`、`/v1/messages` 和 Codex 模型目录生成，拥有上下文窗口、思考档位与工具元数据。
- 图像生成模型只用于 `/v1/images/generations`，拥有输入输出模态、尺寸、质量、背景、输出格式与编辑能力。

服务启动时读取目录。默认路径为仓库的 `deploy/app/config`；生产镜像通过 `PRELAY_CATALOG_DIR=/app/config` 读取只读挂载目录。目录无效时服务拒绝启动。

## 配置目录

配置目录包含三份文件：

```text
models/language.toml
models/image-generation.toml
providers.toml
```

它们不包含 API Key、Endpoint Token、设备凭据或数据库内容。

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

官方资料未明确的字段以英文键名注释保留。`reasoning_efforts` 只能使用 `none`、`minimal`、`low`、`medium`、`high`、`xhigh` 或 `max`；非空时必须同时给出其中一个 `default_reasoning_effort`。

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

## 接口边界

认证后的 `GET /api/provider-catalog` 返回三个并列数组：

```text
language_models
image_generation_models
providers
```

供应商响应同样使用 `language_models` 与 `image_generation_models`，不再返回混合的 `models` 数组。语言模型响应保留完整 Codex 目录元数据；图像生成模型响应只返回图像生成字段。

接入点模型列表也按调用类别拆分：

| 路径 | 内容 |
| --- | --- |
| `GET /v1/models` | 当前接入点已配置的语言模型 |
| `GET /v1/images/models` | 当前接入点已配置的图像生成模型 |
| `POST /v1/images/generations` | 仅解析图像生成模型候选 |

`/v1/responses`、`/v1/chat/completions` 与 `/v1/messages` 仅解析语言模型候选。服务端按候选的上游模型 ID 在启动时加载的目录中判定类别，因此同一接入点别名不会同时进入语言与图像生成链路。

## 目录生成

Codex 的 `models.json` 只应消费 `language_models`。图像生成模型不生成 Codex 模型目录项，也不携带语言模型的空字段占位。

目录不热重载。修改任一 TOML 文件后需要重启服务，使模型分类、供应商引用与接口输出在同一个启动配置中生效。

# Images Generations 协议

## 目标

Prelay 通过接入点向内网客户端提供 OpenAI 兼容的图像生成入口：

```text
POST /v1/images/generations
```

调用仍由 Endpoint Token 授权。服务端根据接入点的模型映射选择上游供应商，替换为对应的上游模型名后转发请求。Provider API Key 只在服务端解密使用，不能出现在客户端状态、协议示例、请求记录或日志中。

## 供应商协议

Images Generations 是第四种上游协议，稳定配置值为 `images_generations`：

```text
responses
openai
anthropic
images_generations
```

它与 `responses`、`openai`、`anthropic` 一样由供应商配置声明，而非由服务端按供应商品牌判断。供应商的 `upstream_protocols` 包含 `images_generations` 时，才可处理图像生成请求；对应的 `protocol_base_urls.images_generations` 是该协议的上游 Base URL。

客户端的 GoToken 预设默认声明该协议并设置其 OpenAI 兼容 Base URL。其他预设默认不声明。任何拥有兼容接口的供应商都可在管理台配置此协议和地址。

已有供应商配置不做数据库或数据迁移。需要图像生成的既有供应商应通过管理 API 或桌面管理台保存 `images_generations` 配置。

## 请求与响应

服务端只要求请求 JSON 中存在字符串 `model`。除此之外，包含 `prompt`、`size`、`quality`、`output_format`、`response_format`、`stream` 和 `partial_images` 在内的字段均不解析、不转换，原样转发至上游：

```text
<images_generations Base URL>/images/generations
```

非流式响应直接透传 JSON。`stream: true` 时，服务端保持上游 SSE 的事件与分块顺序，不等待完整图像，不重编码 Base64，也不转换部分图像事件。服务端不保存图片 URL、Base64、图片字节或提示词。

上游连接失败、5xx、429 和 408 沿用现有候选供应商重试和切换策略；鉴权、模型或其他非瞬态错误直接返回。接入点没有符合该协议的模型时，返回明确的请求错误。

## 记录与隔离

请求记录使用 `images_generations` 标识输入、输出和上游协议，记录接入点、请求模型、上游模型、供应商、HTTP 状态、耗时、上游请求 ID 和脱敏错误摘要。图像内容、返回 URL、Base64、提示词和 Provider API Key 均不进入记录。

接入点鉴权和模型候选查询复用现有身份隔离链路。Endpoint Token 只能访问所属身份及接入点配置的模型，不能通过图像生成入口跨身份访问供应商或模型。

## 协议材料与验证

公开协议示例的唯一来源是 `prelay-protocol/docs/protocol/`。实现时在该集合新增图像生成 Bruno 请求，环境文件继续只使用占位符。

验证覆盖：

- 未携带或携带错误 Endpoint Token 时拒绝请求。
- 只从已声明 `images_generations` 的接入点模型候选中选择上游。
- 对上游模型名的替换、非流式 JSON 透传和 SSE 首包透传。
- 上游失败、流中断和请求记录的脱敏边界。
- 两个身份使用相同对外模型名时的接入点隔离。

# Prelay 服务端接口文档

本目录是服务端的 Bruno 接口集合。`供应商`、`接口`、`身份` 和 `统计` 分别按实际资源组织；每个 Bruno 文件对应一个已注册的路由方法。

## 使用方式

在 Bruno 中打开本目录。复制 `environments/Development.bru` 为个人环境后填写服务地址。每个请求中的尖括号占位值应按该请求填写；个人环境及真实凭据不得提交到仓库。

## 调用顺序

1. 在 `身份/注册.bru` 注册身份。
2. 在 `供应商/新增.bru` 新增供应商，并将响应中的 `id` 填入后续供应商或接口请求的 `<provider-id>`。
3. 在 `接口/新增.bru` 新增接口，并将响应中的 `id` 和 `token` 分别填入后续请求的 `<endpoint-id>` 与 `<endpoint-token>`。
4. 调用根目录的 `模型列表.bru`，将返回模型的 `id` 填入协议请求的 `<endpoint-model-name>`。
5. 按客户端协议调用 `创建响应.bru`、`创建对话补全.bru` 或 `创建消息.bru`。

## 鉴权

- `身份/注册.bru` 无需鉴权。
- 其余身份、供应商、接口和统计请求使用请求内的 `<device-credential>`。
- 根目录的协议请求使用请求内的 `<endpoint-token>`，可通过 `Authorization: Bearer` 或 `X-Api-Key` 传递。

供应商密钥只由供应商创建、更新、模型发现和协议测试请求接收。持久化后服务端仅返回脱敏的 `api_key_masked`，接口令牌不能替代供应商密钥，供应商密钥也不能调用协议入口。

`供应商/Ping.bru` 只检查已保存供应商的 `base_url` 是否可达，不使用供应商密钥，也不验证模型或协议。

## 请求记录响应

`统计/请求记录.bru` 返回当前身份的请求记录。每条记录包含：时间、入口协议、上游协议、接入点名称、供应商、模型、状态、HTTP 状态、错误码和错误信息、流式标记、输入/输出 Token、缓存读/写 Token、首 Token 耗时、总耗时、上游请求 ID 与元数据 JSON。

缓存 Token 只取上游响应中的 usage：OpenAI 兼容响应使用 `prompt_tokens_details.cached_tokens`（兼容 `cache_read_input_tokens`），Responses 响应也接受 `input_tokens_details.cached_tokens`，Anthropic Messages 使用 `cache_read_input_tokens` 与 `cache_creation_input_tokens`。上游未返回时对应字段为 `null`。

## 统计总览响应

`统计/总览.bru`、`统计/模型汇总.bru`、`统计/供应商汇总.bru` 和 `统计/Token 使用趋势.bru` 都使用同一个查询参数 `range`。可选值为 `today`、`yesterday`、`this_week`、`last_week`、`this_month`、`last_month`、`this_year`、`last_year` 与 `all`；省略时默认 `today`。所有范围均按北京时间自然日边界聚合。

总览中的请求数、成功/失败数、输入/输出 Token、缓存读/写 Token 与平均响应耗时都只统计所选范围。`average_latency_ms` 是所选范围内请求总耗时的平均值，单位为毫秒；没有请求记录时为 `null`。

缓存读写 Token 直接汇总请求记录中的 `cache_read_tokens` 与 `cache_write_tokens`。客户端的缓存命中率按 `cache_read_tokens / (input_tokens + cache_read_tokens)` 计算；当分母为零时不显示比例。

## Token 使用趋势响应

`统计/Token 使用趋势.bru` 调用 `GET /api/stats/timeline?range=today`，返回当前身份所选范围的输入、输出、缓存读和缓存写 Token。趋势桶为：今日或昨日按小时，本周、上周、本月、上月按天，本年、去年和总计按月。服务端补齐范围内没有请求的桶为零值；总计从当前身份首条请求所在月开始汇总。

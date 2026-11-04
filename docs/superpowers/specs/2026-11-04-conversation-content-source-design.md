# 会话内容事件设计

> 本文取代 `2026-10-29-activity-rollout-content-dedup-design.md` 中的会话事件格式，并取代 `2026-08-31-activity-memory-design.md` 中关于活动正文采集和记忆来源正文的部分。记忆的长期模型仍可复用。

## 目标

每个线程只有一份会话内容事件流，同时服务会话展示、团队记忆和后续审计。内容只保存一份，事件顺序和模型实际看到的上下文顺序都可以从同一份文件还原。

不再维护协议原始副本、规范化副本和记忆正文副本。

## 核心决定

- 继续使用每线程一份 append-only JSONL。
- 文件头和窗口/轮次标记只出现一次，后续事件继承当前窗口和轮次。
- 事件使用统一的精简字段，不为每行重复写 `schema_version`、`thread_id`、`turn_id`、`window_id`。
- 时间使用带时区的可读 RFC 3339。
- 历史回放只保存有序引用，不重复保存正文。
- 图片、文件、音频只保留类型占位，不保存二进制或 base64。
- 不可读或纯协议控制字段不进入会话内容。
- 会话展示、记忆库和审计读取同一份事件流。

## 非目标

- 不保存完整协议请求体、响应体、HTTP 头和 SSE 中间帧。
- 不保存 `item_reference`、`configuration_update`、compaction token 等不含可读内容的控制项。
- 不保存只有 `encrypted_content` 的 reasoning。
- 不保存图片、音频、文件内容。
- 不建立第二份记忆正文或独立审计正文。
- 不引入二进制格式。

## 文件布局

继续使用现有线程路径：

```text
data/sessions/<identity_id>/<thread_id>.jsonl
```

一个线程一个文件。索引文件与该 JSONL 同目录，只保存定位、校验和计数状态，不保存正文。

## 事件格式

文件首行是元数据：

```json
{"v":1,"type":"meta","thread_id":"...","created_at":"2026-11-04T15:29:58.001+08:00"}
```

生成窗口时写入窗口标记：

```json
{"seq":1,"time":"2026-11-04T15:30:00.100+08:00","type":"window","window_id":"...:0","generation":0}
```

每次请求写入轮次标记：

```json
{"seq":2,"time":"2026-11-04T15:30:00.110+08:00","type":"turn","turn_id":"turn_...","activity_id":"...","model":"deepseek-flash","upstream_model":"deepseek/deepseek-v4.1-flash"}
```

内容事件使用同一顶层结构，`payload` 按 `type` 区分：

```json
{"seq":3,"time":"2026-11-04T15:30:00.123+08:00","type":"message","payload":{"phase":"request","role":"user","content":"..."}}
{"seq":4,"time":"2026-11-04T15:30:02.456+08:00","type":"reasoning","payload":{"phase":"response","content":"..."}}
{"seq":5,"time":"2026-11-04T15:30:02.500+08:00","type":"tool_call","payload":{"phase":"response","tool_call_id":"call_...","name":"exec_command","arguments":{}}}
{"seq":6,"time":"2026-11-04T15:30:03.200+08:00","type":"tool_result","payload":{"phase":"request","tool_call_id":"call_...","content":"..."}}
{"seq":7,"time":"2026-11-04T15:30:03.300+08:00","type":"usage","payload":{"input_tokens":123,"output_tokens":45}}
{"seq":8,"time":"2026-11-04T15:30:03.400+08:00","type":"turn_end","payload":{"status":"completed"}}
```

字段规则：

- `seq` 是线程内稳定事件 ID，后续引用使用 `seq`，不再另造 `event_id`。
- `time` 固定使用 RFC 3339，带 `+08:00` 时区，保留毫秒。
- `type` 使用可读字符串，不压缩成数字编码。
- `payload` 字段按事件类型定义，公共字段只在文件头、窗口标记或轮次标记中出现。
- 缺省值和空字段不写。

## 事件类型

### message

保存 system、developer、user、assistant 的文本内容。

`payload` 字段：

- `phase`：`request` 或 `response`
- `role`：`system`、`developer`、`user`、`assistant`
- `message_phase`：assistant 的 `commentary` 或 `final_answer`
- `content`：字符串，或 content part 数组

文本 content part 保留原文。图片、文件、音频只保留占位：

```json
{"type":"input_image","omitted":true}
```

assistant refusal 作为内容保存：

```json
{"type":"refusal","text":"..."}
```

### reasoning

保存上游明确返回的可读 reasoning summary 或 reasoning content。

`payload` 字段：

- `phase`
- `summary`（可选）
- `content`（可选）

两者都存在时分别保存，不互相覆盖。

只有 `encrypted_content`、没有可读文本的 reasoning 不写事件。

### tool_call

保存所有工具调用，不只 function call。

`payload` 字段：

- `phase`
- `tool_call_id`
- `name`：function 名或官方工具类型
- `arguments`

`arguments` 可以是字符串或 JSON 对象，保存工具的输入、action 或其他原始参数结构。

### tool_result

保存工具结果正文。

`payload` 字段：

- `phase`
- `tool_call_id`
- `content`

工具结果中的图片、文件、音频同样只保留占位。

### usage

保存本轮 token 用量。

### error

保存错误、取消、超时和中断。

`payload` 字段：

- `code`
- `message`
- `http_status`

### turn_end

保存本轮最终状态：

- `status`
- `http_status`
- `error_code`（可选）

### context

`context` 保存模型本轮实际看到的完整输入顺序：

```json
{"seq":20,"time":"2026-11-04T15:30:04.000+08:00","type":"context","payload":{"refs":[3,4,5,6]}}
```

`refs` 是有序的 `seq` 列表。新输入内容先写成内容事件，再写 `context` 引用。历史回放不重复写正文，只引用已有内容事件。

`context` 是审计事件，不进入普通会话展示，也不作为记忆内容。

## 顺序与回放

文件中的物理顺序是写入顺序，`context.refs` 是模型输入的逻辑顺序。

每个请求执行：

1. 写 `turn` 标记。
2. 对本轮请求中的内容项做匹配。
3. 未匹配的内容项写新内容事件。
4. 写 `context`，按本轮实际输入顺序列出所有内容项的 `seq`。
5. 写本轮新增的响应内容、reasoning、工具调用、工具结果和 usage。
6. 写 `turn_end`。

新增的相同内容消息仍是新事件；只有确认是历史重放的内容项才只写引用。

窗口 generation 变化时写新的 `window` 事件。旧窗口内容不复制到新窗口，`context.refs` 可以跨窗口引用旧事件。

## 去重

内容事件只保存一份正文。历史重放只产生 `context.refs`。

匹配顺序：

1. 如果有稳定 `item_id` 或 `tool_call_id`，按 ID 匹配。
2. 没有稳定 ID 时，按内容身份和当前窗口中已出现的次数匹配。
3. 超出已出现次数的相同内容视为新事件。

这样既不会把重复历史正文写入文件，也不会把用户真实重复发送的同一句话误判成历史。

## 附件

图片、音频、文件不保存正文、base64、hash 或文件名。只在消息内容中保留 `type` 和 `omitted` 占位，保证消息结构不缺失。

如果后续需要恢复附件，应通过独立的对象存储能力实现，不改变本事件流的唯一数据源边界。

## 读取

### 会话展示

按 `seq` 读取 `message`、`reasoning`、`tool_call`、`tool_result` 和 `error`。忽略 `context`、`turn`、`turn_end` 等控制事件。

历史内容只在原事件位置展示，不因为 `context` 引用而重复展示。

### 记忆

记忆库读取同一份内容事件，使用 `seq` 作为来源标识。`context`、`usage`、`turn_end` 不作为记忆正文。

记忆索引不保存第二份会话正文。

### 审计

审计读取同一份内容事件。需要还原模型输入时，按 `turn` 和 `context.refs` 展开顺序；需要查看输出时，按轮次后的内容事件读取。

## 索引

线程索引升级版本，至少保存：

- 文件长度和修改时间
- 行数、末序号
- 当前窗口和 generation
- 内容身份及出现次数
- 内容事件计数

索引不保存正文，损坏或版本不匹配时回退重建。

## 兼容

旧 JSONL 没有文件头格式标记，按 legacy 读取。旧数据中已经缺失的 opaque item、refusal、内置工具调用和附件不能补回。

新文件从 `v=1` 开始使用本文格式。新代码必须同时支持 legacy 读取和 v1 写入；旧索引失效后重建。

## 验收

- 同一线程只有一个内容事件流，展示、记忆、审计读取同一份文件。
- 历史回放不重复保存正文，只保存 `context.refs`。
- 相同内容的真实新消息可以正常追加。
- reasoning、所有工具调用、工具结果、错误和 usage 都有对应事件。
- 图片、文件、音频不保存正文，只保留占位。
- 每行不重复写线程、窗口、轮次和格式版本。
- 时间字段可直接被人和大模型阅读。
- 旧数据缺失的内容不会被伪造。

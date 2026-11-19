# 记录日志与异步物化设计

> 本文定义请求记录从同步多写改为 WAL 提交加异步物化的目标结构。`/v1` 协议、接入点解析、故障迁移、活动统计口径和会话内容格式保持不变。

## 实现状态

本设计已经进入服务端生产请求路径，当前代码状态如下：

- 非流式 `/v1/responses`、桥接 Chat Completions、原生 `/v1/chat/completions` 和图片生成使用 `TurnCompleted`，成功响应等待 WAL 组提交 `fsync`。
- 流式 Responses、Chat Completions 和图片生成在首块前提交 `TurnStarted`，终止事件、usage、response state 与最终状态写入 `TurnFinished`；稳态不按 chunk 写 WAL。
- `ProviderObservation` 已替代请求路径中的 `remember_protocol_model_provider` 同步写库，物化失败只保留旧路由，不阻塞活动记录。
- response state 在 WAL 提交后进入 pending cache，物化成功后移除；下一轮 `previous_response_id` 不再依赖物化完成时序。
- 启动时先回放已提交 WAL，再恢复旧 `writing` rollout，然后才启动后台物化并接受请求；中间损坏、解密失败和 journal unhealthy 会阻断新请求。
- `GET /health` 暴露 `ready`、`degraded`、`blocked`、durable sequence、已应用 sequence、物化延迟和 pending response state 数量。
- WAL 已应用段在 checkpoint 持久化后清理；容量阈值、最小剩余空间、组提交超时和优雅关闭已接入运行时。
- Docker Compose 已增加 `./data/recording` 持久卷，部署环境变量和备份说明已同步到 `.env.example` 与 README。

当前实现没有引入 `RECORDING_PIPELINE` 运行时切换开关；生产启动始终创建 `RecordingRuntime`，旧同步记录分支只保留在测试桩构造的 `recording: None` 场景，不会在正式部署中与 WAL 双写。

真实生产 PostgreSQL、持久卷、进程终止和掉电模拟仍需在部署环境执行；这些是发布验收项，不是请求路径中保留的第二套记录链路。

## 背景

当前请求路径同时承担推理转发和记录持久化：

- 非流式请求在上游响应完成后，先等待 `save_response_state`，再等待 `record_completed` 完成 rollout 写入、`flush`、`sync_all` 和数据库事务，之后才返回响应。入口见 `src/inbound/v1/responses/native.rs`、`src/inbound/v1/responses/chat.rs`。
- 流式请求在首块前执行 `begin_stream`，读取线程索引并写请求事件；拿到上游首块后执行 `start_stream`，插入活动与 rollout 索引，之后才把首块转发给客户端。`remember_protocol_model_provider` 也在响应返回前写数据库。
- 流式结束时，final usage、response state、rollout `finish_with` 和 `complete_stream` 仍可能位于最后一块或流关闭之前。
- `RolloutWriter` 使用同步 `std::fs`，每行 `flush`，每个活动结束时 `sync_all`。`sync_all` 和文件读写运行在异步请求路径上，会占用 Tokio worker。
- `first_token_ms` 在 `start_stream` 落库前计算，`latency_ms` 在 `record_completed` 或 `finalize_rollout` 前计算。现有指标低估客户端真实等待。

这些写入不是同一个原子操作。活动元数据、线程 JSONL 和 response state 分散在数据库与文件系统中，任何一处变慢都会进入请求延迟。继续在调用点增加异步任务只会形成多套临时补偿，不能解决状态所有权和恢复语义。

## 与现有会话内容设计的关系

会话正文的事件类型、窗口/轮次规则、历史重放去重和每线程一份 JSONL 的格式，以 `docs/superpowers/specs/2026-11-04-conversation-content-source-design.md` 为准。本文只改变这些事件的写入时机、提交边界和恢复方式，不重新定义事件格式，也不引入第二份正文来源。

## 目标

- 将数据库事务、线程索引读取和线程 JSONL 物化移出请求首字节与成功返回路径；请求只等待 WAL 顺序写和组 `fsync`。
- 已向客户端确认成功的请求，在进程崩溃、重启和掉电后不丢记录。
- 流式请求首块不被数据库插入阻塞；终止成功事件只在记录提交后发送。
- 记录的最终格式、统计口径、接入点解析、故障迁移和上游调用行为不变。
- 用一个可恢复的提交边界统一活动、会话正文和 response state，而不是分别优化。
- 空库、空 WAL、已有历史数据、中断流和部分 rollout 都有确定行为。

## 非目标

- 不引入直连模式、客户端数据面或客户端记录代理。
- 不修改 `/v1` 请求响应协议、Endpoint Token、Provider Key 共享模型和故障迁移策略。
- 不把会话正文迁入 PostgreSQL，不改变每线程一份 JSONL 的内容格式。
- 不改统计维度、身份隔离、清理策略和管理 API DTO。
- 不在本期实现多副本共享日志或跨节点接管。本文以单实例数据面为部署前提。
- 不保留长期双写。迁移期可以存在短期回滚开关，最终结构只保留 WAL 提交路径。
- 不以降低总磁盘写入量为目标。WAL 保存内容后还要物化到线程 JSONL，会产生写放大；本设计优先解决请求路径延迟和已确认请求的持久性。若磁盘吞吐是主要瓶颈，本设计不适用，需要另行评估元数据 WAL 或 staging 方案。

## 设计原则

- 请求处理器只构建记录并提交给记录日志，不再直接调用活动、rollout 或 response state 的写接口。
- WAL 在物化完成前是记录的权威来源；物化完成后，PostgreSQL 索引和线程 JSONL 是查询与展示的权威来源。
- 客户端确认成功必须晚于 WAL 提交。记录不能以“尽力而为”的方式伴随成功响应返回。
- 物化至少执行一次，通过活动 ID、response ID 和线程内活动检测实现幂等。
- 同一线程的 rollout 写入保持顺序；不同线程可以并行物化。
- 任何容量不足、提交失败或 WAL 损坏都必须显式失败或进入阻断状态，不能静默丢记录。
- 临时兼容路径必须有明确退出条件，不能成为第二套长期记录链路。
- 记录发生时间必须来自请求/响应观测，不得用物化时的当前时间替代。
- 启动恢复只有一个责任方；WAL 回放、旧中断恢复、身份清理和 HTTP 接流量必须由同一个协调顺序驱动。
- WAL 的收益是请求延迟和持久性，不承诺降低总磁盘写入量；写放大必须作为部署和压测指标。

## 方案取舍

- **请求内 `spawn` 异步任务**：响应最快，但任务只存在内存中，进程崩溃或掉电直接丢记录；没有统一提交边界，不能作为数据服务的持久化方案。
- **数据库 outbox**：多副本共享、恢复简单，但每个请求仍在关键路径上写数据库；大正文进入数据库会放大事务、WAL 和备份压力，和“把记录从请求路径移走”的目标冲突。适合未来多副本 HA 后端，不作为当前单实例首选。
- **本地 WAL + 组提交**：单次顺序追加、跨请求摊销 `fsync`，延迟最低；持久卷上可回放，符合当前单实例数据面。代价是节点永久丢失时未物化记录不能由其他副本接管，作为部署边界明确列出。
- **外部消息队列**：解耦能力强，但引入额外部署、幂等消费和运维面；当前规模不需要。

## 目标架构

```text
inbound /v1 handlers
        |
        v
RecordingJournal
        |
        +--> WAL segment append
        |        |
        |        v
        |   GroupCommitter -> commit frame -> fsync
        |        |
        |        v
        |   committed sequence watermark
        |
        v
RecordingMaterializer
        |
        +--> ensure PostgreSQL activity / rollout index
        +--> append thread JSONL through RolloutStore
        +--> update rollout summary and response state
        +--> provider observation
        |
        v
checkpoint -> segment cleanup
```

请求处理器只等待 WAL 提交回执，不等待数据库和线程 JSONL 物化。物化延迟由后台 worker 吸收，WAL 积压通过容量阈值和健康状态暴露。

### 组件职责

| 组件 | 职责 | 不负责 |
| --- | --- | --- |
| `inbound/v1` 处理器 | 生成 `activity_id`，构建请求/响应事件、用量、状态和 response state，调用记录日志 | 数据库写、rollout 文件写、`sync_all`、重试 |
| `RecordingJournal` | 编码逻辑记录、分片、加密、追加 WAL、分配序号、返回提交回执 | 业务格式转换、线程索引、数据库事务 |
| `GroupCommitter` | 批量提交逻辑记录、追加 commit frame、执行 `fsync`、发布 durable watermark | 物化、查询、清理 |
| `RecordingMaterializer` | 读取已提交记录，写入 PostgreSQL 和线程 JSONL，更新索引和 response state | 处理请求、生成上游请求、决定响应 |
| `RecordingRecovery` | 启动时校验 WAL、回放未应用记录、协调旧中断恢复和身份清理、标记中断活动、重建缓存 | 接受新请求前绕过恢复 |
| `RecordingRuntime` | 持有 journal、materializer、recovery，管理启动顺序、优雅关闭和健康状态 | 业务请求处理、Provider 调用 |
| `ActivityStore` / `RolloutStore` | 提供现有数据库和线程 JSONL 适配能力 | 直接承接请求路径的同步记录 |
| response state cache | 提供下一轮 `previous_response_id` 的快速读路径 | 替代数据库或 WAL 的持久化 |

### 状态所有权

- `activity_id` 在请求处理器生成，是 WAL、`identity_activities`、`activity_rollouts` 和线程 JSONL 的统一幂等键。
- WAL `sequence` 在单个 `node_id + epoch` 内单调递增，是提交与回放的顺序依据。
- `durable_sequence` 由 `GroupCommitter` 在 commit frame `fsync` 成功后发布。
- `last_applied_sequence` 由 `RecordingMaterializer` 在完整逻辑记录物化后推进，保存到 checkpoint。
- 线程 JSONL 中的 `turn.activity_id` 是线程文件内的活动身份；物化重放不得为同一活动追加第二个 turn。
- `identity_response_sessions` 以 `(identity_id, response_id)` 为幂等键；同一 WAL 记录重放只能 upsert，不能重复插入。
- `RecordingMaterializer` 维护 pending identity 集合；身份清理不能删除 pending 集合中的身份，直到相关 WAL 越过 checkpoint。

## 记录模型

WAL 使用逻辑记录而不是逐数据库操作。每个逻辑记录包含完整的可物化信息。

| 记录类型 | 使用场景 | 内容 |
| --- | --- | --- |
| `TurnCompleted` | 非流式成功或失败，或流式候选在首块前失败 | 活动元数据、会话上下文、请求事件、响应事件、用量、延迟、状态、response state |
| `TurnStarted` | 流式开始 | 活动元数据、请求事件、会话上下文、endpoint/provider/model、流式标记 |
| `TurnFinished` | 流式成功、失败、断开或超时 | 最终状态、用量、首字节/总延迟、响应事件、response state、会话上下文引用 |
| `ProviderObservation` | 接入点模型最近使用的 Provider | endpoint、模型、Provider ID |

`TurnCompleted` 是非流式的完整记录。流式必须按 `TurnStarted`、`TurnFinished` 顺序提交；只有 `TurnStarted` 的记录在恢复时视为中断活动。`ProviderObservation` 是低优先级观察，不参与响应成功判定。

所有会产生活动的 `/v1` 路径都走同一记录管线：`/v1/responses`、`/v1/chat/completions` 和 `/v1/images/generations`。`/v1/models` 只读模型目录，不产生活动。图片流式路径当前是 best-effort 记录，目标结构统一为首块和终止事件两个提交边界；图片二进制仍按现有会话内容规则只保留占位，不进入 JSONL。

### 与现有记录入口的映射

| 现有入口 | 目标逻辑记录 |
| --- | --- |
| 非流式 `record_completed` + `save_response_state` | `TurnCompleted` |
| 流式 `begin_stream` | `TurnStarted` |
| 流式 `record_stream_end` / `complete_stream` / response state 保存 | `TurnFinished` |
| `remember_protocol_model_provider` | `ProviderObservation` |

### 记录语义

- 活动记录表示服务端观测到一次上游调用及其最终状态，不等价于客户端一定收到了响应。
- WAL 提交成功但响应未送达时，服务端仍可能留下成功活动；客户端重试会产生新的 `activity_id`。
- 已向客户端确认成功的请求必须已经包含在成功的 commit 帧中；未确认请求不承诺存在记录。

### 候选尝试与活动

- 每个上游候选尝试独立生成 `activity_id`。一次客户端请求经过 failover 时，可能产生多个失败活动和一个最终活动。
- 失败候选的 `TurnCompleted` 或 `TurnFinished` 先提交 WAL，再决定是否继续下一个候选；活动顺序与候选尝试顺序一致。
- WAL 提交失败时 fail closed，不再尝试下一个候选，因为后续成功也无法可靠记录。
- 失败候选不生成 response state；只有最终成功或确实产生可续接状态的尝试才包含 response state。

逻辑记录字段：

```json
{
  "schema_version": 1,
  "record_id": "activity-id:turn_completed",
  "activity_id": "activity-id",
  "identity_id": "identity-id",
  "kind": "turn_completed",
  "created_at": "2026-11-15T12:00:00.000+08:00",
  "part_count": 1
}
```

大记录按 `part_index` 和 `part_count` 分片。每个分片独立加密和校验，物化器按顺序重组。所有分片必须在同一个提交组内提交，避免出现半条逻辑记录被视为成功。

### 时间语义

WAL 必须携带记录发生时间，物化器不得重新调用 `Utc::now()` 生成活动或会话时间。

逻辑记录至少包含：

- `started_at`：本轮请求开始时间，写入 `identity_activities.created_at`、`activity_rollouts.created_at`、线程 `meta` 和 `turn` 事件。
- `first_token_at`：首个下游内容块交付时间；未知时允许为空。
- `finished_at`：本轮终止时间，写入 `activity_rollouts.completed_at`、`usage`、`error` 和 `turn_end` 事件。
- 每个内容事件必须携带 `observed_at`：请求阶段事件使用 `started_at`，响应事件由采集路径提供，缺失时使用 `finished_at`。物化器不得生成 `observed_at`。

实现需要为 `ActivityStore`、`Storage` 和 `RolloutWriter` 提供显式时间参数，禁止在物化时隐式取当前时间。延迟物化测试必须证明：WAL 写入后人为延迟数分钟，活动时间线、统计分桶、线程 `meta`/`turn`/`turn_end` 时间和审计事件时间仍等于原始请求时间。

## 行为变化

- 成功响应和流式终止完成事件增加 WAL 提交屏障；提交失败返回 503 或流错误，不再发送成功。
- `store=true` 的 response state 写入失败不再静默继续，避免下一轮续接失败。
- 候选失败记录提交失败时停止 failover，因为后续成功也无法可靠记录。
- 图片流式记录从 best-effort 改为与语言模型相同的提交边界。
- `first_token_ms` 和 `latency_ms` 改为包含真实下游首字节和 WAL 提交等待；与旧指标不可直接横向比较。
- 活动列表、统计和会话正文读取在物化完成前可能短暂滞后；延迟上限由 materialization lag SLO 约束，不能无限期缺失。
- 会话事件时间和活动 `created_at` 继续表示原始请求时间，不表示物化时间。
- 总磁盘写入增加，监控必须同时看 WAL 字节、rollout 字节和写放大比例，不能只看请求延迟。
- `/v1` 协议、DTO、活动查询结构和会话内容格式不变；客户端可能看到更多记录基础设施导致的 503。

## WAL 格式

### 目录与段

默认目录由 `RECORDING_JOURNAL_DIR` 指定，放在持久卷中，不使用容器临时层。

```text
data/
  recording/
    node-id
    checkpoint.json
    wal-<node_id>-<epoch>-000001.log
    wal-<node_id>-<epoch>-000002.log
```

- `node-id` 是记录日志写入实例的持久身份，不是客户端 `machine_id`、Provider ID 或主机名；在目录首次初始化时生成并持久化。
- 已有 WAL 但 `node-id` 缺失、段名不一致或无法验证时，恢复阻断；不得重新生成后继续使用旧 WAL。
- `epoch` 是每次服务进程启动生成的随机唯一标识（UUIDv7 或 ULID），不是递增整数；旧 epoch 的未应用段先回放，再创建新段。随机 epoch 避免数据目录克隆或同起点恢复时产生重复序号。
- 段文件按大小轮转，已应用且已越过 checkpoint 的段可以删除。
- 同一目录只允许一个数据面实例持有写锁；第二个进程必须拒绝启动，不能并发追加同一段。
- 克隆数据目录、从备份恢复到另一实例或复制到另一台机器时，必须生成新的 `node-id`，并把旧 WAL 作为待恢复数据隔离处理；不得让两个实例共享同一 `node-id` 目录。已有 WAL 的克隆目录如果没有独立隔离策略，启动必须阻断。

### 帧

每个帧包含：

- magic、format version、frame length。
- frame sequence。
- frame type：`data` 或 `commit`。
- `activity_id`、record kind、part index、part count。
- payload 长度、payload SHA-256、帧 CRC。
- 加密 nonce 和密文。

`data` 帧承载逻辑记录分片。`commit` 帧只承载本次提交的 `group_id`、`first_sequence`、`last_sequence` 和提交时间，不包含业务正文；commit 帧同样使用 journal key 做认证，避免本地篡改提交范围。提交顺序固定为：追加 data 帧，追加 commit 帧，`fsync`，发布 durable watermark。

单个逻辑记录可以跨段；commit 帧位于该逻辑记录最后一段。帧序列在 `node_id + epoch` 内全局单调，跨段不回退。

恢复只应用被有效 commit 帧覆盖的 data 帧。末尾未提交或被掉电截断的 data 帧丢弃；中间 CRC 错误不跳过，服务进入恢复阻断状态。

### 加密与数据边界

WAL 可能包含提示词、模型输出和 response state，按敏感数据管理。

- 使用 AES-256-GCM 加密每个 data 帧 payload。
- journal key 从 `ENCRYPTION_KEY` 通过 HKDF-SHA256 派生，salt 固定为 `prelay-recording-wal-salt`，info 固定为 `prelay-recording-wal-v1`。
- AAD 绑定 `node_id`、`epoch`、`frame_sequence`、`activity_id`、record kind、part index 和 part count，防止重排和跨记录替换。
- commit 帧不包含业务正文，只包含序列范围和校验信息。
- WAL 目录和文件继承数据目录的服务账户 ACL，只允许服务账户读写；日志、追踪和错误信息不得输出 payload。
- 轮换 `ENCRYPTION_KEY` 前必须排空 WAL。存在未应用 WAL 时禁止更换主密钥，不能靠跳过解密启动。
- 最终线程 JSONL 继续遵循现有文件数据边界；WAL 加密不改变 rollout 文件的保护方式。

### checkpoint

`checkpoint.json` 保存：

```json
{
  "node_id": "node-id",
  "epoch": "0192f7d4-7f5b-7c31-9d8e-4f0f6b2a9c10",
  "segment": 2,
  "offset": 1048576,
  "last_applied_sequence": 9182,
  "updated_at": "2026-11-15T12:00:03.000+08:00"
}
```

checkpoint 通过临时文件、`fsync` 和原子替换写入。checkpoint 损坏或落后时，从更早的段回放；物化幂等保证重复应用不产生重复数据。

## 提交边界

### 非流式

1. 上游响应完整返回。
2. 构建 `TurnCompleted` 逻辑记录。
3. 追加分片和 commit 帧，等待组提交 `fsync`。
4. 提交成功后返回响应。

提交失败时返回 503，不发送成功响应。上游请求已经发生的成本由调用方重试策略承担，不能用“已收到上游响应”替代记录成功。

### 流式

1. 建立上游响应的流式读取。
2. 构建并提交 `TurnStarted`；提交成功后才发送首块。
3. 流式过程中只在内存中捕获内容，不按 chunk 写 WAL。
4. 终止事件、final usage、response state 和最终状态构建 `TurnFinished`。
5. 提交 `TurnFinished` 成功后发送终止完成事件并关闭流。

如果上游错误、客户端断开或超时，尽可能构建 `TurnFinished` 并记录 `failed` 或 `partial`；该记录不阻塞已经发送给客户端的错误路径，但必须在流关闭前尝试提交。进程在流中被终止时，恢复阶段根据只有 `TurnStarted` 的活动标记为 `partial`。

`store=true` 的 response state 与活动终态同组提交。response state 无法构建或写入时，不发送成功完成事件；`store=false` 不生成 response state。这样不会出现“客户端拿到成功响应但下一轮 `previous_response_id` 找不到”的状态。

### 组提交

- 默认每 2 毫秒或累计 64 条逻辑记录触发一次组提交；达到最大字节阈值立即提交。
- 2 毫秒是最大等待时间，不是固定延迟；没有其他可合并记录时，首条记录可以立即提交。
- 同一逻辑记录的所有分片放在同一个提交组。
- 单个逻辑记录超过组字节阈值时单独成组，不拆分到多个提交组；提交组可以跨越 WAL 段。
- 等待者只在 commit 帧 `fsync` 成功后收到回执。
- `fsync` 失败或超过 `RECORDING_JOURNAL_COMMIT_TIMEOUT_MS` 时，失败组的所有等待者收到提交错误；journal 进入 unhealthy 状态，新的提交被拒绝，直到重启恢复或显式修复。
- 组提交间隔只影响等待时间，不影响已确认请求的持久性。已确认请求必须已包含在成功的 commit 帧中。

## 物化

物化器只消费已提交记录，按 `durable_sequence` 顺序读取，并按线程分区并行。

数据库表结构不变。需要为 `ActivityStore` / `Storage` 增加接受外部 `activity_id`、`started_at`、`finished_at` 的幂等占位、终态更新和 response state upsert 接口；需要为 `RolloutStore` 增加按 `activity_id` 查询已有 turn 的只读接口和显式事件时间写入接口。处理器不再使用这些写接口。

线程 JSONL 的物化在专用阻塞线程中执行，不占用请求路径的 Tokio worker。文件按事件批量缓冲，活动结束执行一次 `sync_all`，再更新数据库并推进 checkpoint；不逐行 `flush`，也不把文件同步放到异步请求任务里。

### 线程写入模型

每个 `thread_id` 由一个串行 actor 持有写入顺序，避免把现有异步 `RolloutStore` 锁和同步 `RolloutWriter` 混在一起使用。

- actor 在 Tokio 任务中按 WAL 序号接收该线程的记录，并在同一个 actor 内获取 `RolloutStore` 的线程锁。
- 文件写入和 `sync_all` 通过 `spawn_blocking` 或专用阻塞线程执行；`RolloutWriter` 和它的 `OwnedMutexGuard` 必须作为一个所有权单元移动，不能跨任务拆分。
- 阻塞写入完成后再回到异步任务更新数据库和 checkpoint，不同线程的 actor 可以并行。
- actor 队列有界；队列满时物化器停止消费新记录并触发背压，不能在无界内存中堆积。
- 关闭时停止接收新记录，等待当前线程写入和数据库更新完成；未完成记录保留在 WAL，由下次启动回放。

如果 `RolloutWriter` 的 `Send` 约束或锁生命周期无法满足上述模型，必须把 `RolloutStore` 拆成“异步获取线程上下文”和“阻塞写入”两段，不能把同步 `std::fs` 直接放回 Tokio worker。

### 单活动流程

1. **确保数据库占位**：以 `activity_id` 幂等插入 `identity_activities` 和 `activity_rollouts`；`activity_rollouts.status` 使用 `writing`，流式活动的 `identity_activities.status` 为 `streaming`。已存在时读取当前状态。
2. **确保线程内容**：在对应线程锁内检查该 `activity_id` 是否已经有 turn。没有则通过 `RolloutStore` 追加请求和响应事件；已有则跳过重复写入。
3. **更新数据库终态**：写入 rollout 的起止行、事件数、字节数、SHA-256、最终状态和错误信息。
4. **写入 response state**：按 `(identity_id, response_id)` upsert，先于该逻辑记录标记为已应用。
5. **应用 Provider 观察**：更新接入点模型最近使用的 Provider，重复应用以最后一次为准。
6. **推进 checkpoint**：该活动所有逻辑记录和 response state 都完成后，推进 `last_applied_sequence`。

### 幂等与恢复点

- 数据库已存在且状态为 `completed` 的活动：视为已物化并跳过重复写入；如果线程文件缺失或活动 turn 不存在，不能静默跳过，转入恢复阻断并告警。
- 数据库状态为 `writing` 且线程文件已有完整该活动 turn（含 `turn_end`）：只补齐数据库终态，不重复追加 turn。
- 数据库状态为 `writing` 且线程文件没有该活动：追加完整事件，再更新终态。
- 线程 JSONL 末尾存在半行或半事件时，先截断到最后一个完整换行再处理；中间损坏不自动修复，转入恢复阻断。
- WAL 已有完整 `TurnFinished` / `TurnCompleted`，但线程文件中的 turn 只写了一半：在该 turn 之前最后一个完整事件处截断，按 WAL 重新追加完整 turn；无法安全定位或截断时转入恢复阻断。
- WAL 只有 `TurnStarted`：按中断处理，追加显式的 `turn_end`，状态为 `partial`，错误码 `activity_interrupted`。
- response state 已存在：upsert 覆盖为同一逻辑记录中的稳定值；不因重放产生重复响应。
- `ProviderObservation` 使用独立重试，不阻塞活动记录和 response state 的 checkpoint；失败只影响接入点首选 Provider 的记忆。

### 物化收敛不变量

- 每条记录必须能独立收敛。`TurnStarted` 始终按幂等 upsert 写入活动占位和 rollout 索引，不能因为线程文件里已经出现 `turn_end` 就跳过数据库写入；`TurnFinished` 自带开轮活动快照，活动行缺失时用快照补建该行，再写终态。
- `turn_end` 只结束它所属的 turn。新事件带 `turn_id`，按 `turn_id` 归属；旧事件没有该字段时，只在该 turn 与下一个 turn 之间按位置归属，不能被后续 turn 继承。
- 线程文件的行号必须来自解析结果，不能由事件下标推算。旧格式文件里的 `session_meta`、`media` 等控制行不产生事件，下标与行号并不一一对应。
- 物化失败时 checkpoint 停在最后一个成功批次，并按指数退避重试；已推进 checkpoint 覆盖的 WAL 段即使本轮后续批次失败也要立即回收，避免磁盘只增不减。
- 单轮物化读取按已编码帧累计预算，积压再大也不会整盘读入并解密。预算只在完整 commit 帧之后生效，保证每轮至少推进一个可应用批次；单条记录超过预算时仍完整读取后停止，内存上限约为“预算 + 一条记录”。

### response state 读路径

- WAL 提交成功后，处理器把 `response_id -> WAL locator` 放入 `ResponseStateCache`，标记为 pending；locator 至少包含 record ID、segment、offset 和 length，cache 不复制完整 response state 正文。
- `load_response_state` 先查 locator，命中时从 WAL 解密读取；未命中再查数据库。
- locator 指向的段已清理或读取失败时回退数据库；数据库也无记录时按现有 `previous_response_not_found` 返回。
- 物化器完成数据库 upsert 后移除 locator；数据库写入失败时 locator 保留并重试。
- WAL 提交后 locator 登记失败时，不发送成功完成事件。
- pending locator 不能因普通 LRU 淘汰；受 WAL 容量和内存上限约束。达到上限时拒绝新请求，不能让下一轮静默 miss。
- 启动恢复重建 locator 和数据库可读状态后再接受新请求。

### 线程顺序

同一 `thread_id` 的物化必须串行。调度器按 `thread_id` 哈希分配到固定 worker，worker 内部按 WAL 序号处理；不同线程可以并行。活动级状态和线程文件之间的推进顺序固定为“数据库占位、线程内容、数据库终态”，不能交换。

## 启动恢复

启动顺序固定，恢复完成前 HTTP 健康状态为 `not_ready`，不接新请求。

启动恢复由 `RecoveryCoordinator` 唯一负责。现有 `activity_store.recover_interrupted()`、`warn_incomplete_rollout_files()`、`warn_orphan_rollout_files()` 和身份清理不能再各自按旧顺序独立执行；它们必须被纳入同一恢复流程，或由 WAL-aware recovery 替换。

1. 获取 WAL 目录写锁，读取 `node-id` 和 checkpoint。
2. 从 checkpoint 指定段和偏移开始读取帧，校验 magic、版本、长度、CRC 和加密认证。
3. 只收集被有效 commit 帧覆盖的数据帧；末尾未提交帧截断。
4. 按序号回放逻辑记录，执行物化幂等流程；对只有 `TurnStarted` 的流式活动标记 `partial`，补充中断错误事件；对没有对应数据库行的活动补齐占位和 rollout 索引。
5. WAL 回放完成后，才允许既有 `recover_interrupted()` 扫描剩余 `writing` 记录；已有完整 `TurnFinished` / `TurnCompleted` 的活动不得在回放前被降级为 `partial`。
6. 执行孤儿/不完整 rollout 告警，再执行身份清理。存在 pending WAL 的身份不得被删除；清理器必须读取 materializer 的 pending identity 集合或等待 checkpoint。
7. 所有 `TurnStarted`、`TurnCompleted`、`TurnFinished` 和 response state 回放完成后，才开始接受新请求；积压超过阈值时保持 `not_ready` 并告警。`ProviderObservation` 失败只重试或丢弃该观察，不阻止启动。
8. 创建新随机 epoch 和新段，写入初始 checkpoint，健康状态切换为 `ready`。

恢复失败语义：

- WAL 末尾被掉电截断：截断到最后有效 commit 帧，继续恢复，记录 warning。
- WAL 中间 CRC 或认证失败：停止恢复，健康状态 `blocked`，拒绝新请求，保留现场等待人工处理。
- `ENCRYPTION_KEY` 无法解密已有 WAL：停止恢复，不回退到明文或跳过记录。
- 数据库不可用：保持 `not_ready`，重试连接；不在没有索引的情况下接受成功请求。
- rollout 目录不可写：保持 `not_ready` 或 `degraded`，根据是否已有可读索引决定；不能静默丢弃。

## 运行时生命周期

- `RecordingRuntime` 由服务端 `AppState` 持有，恢复完成并创建 journal 和 materializer 后才启动 HTTP 路由。
- 服务器收到 SIGTERM/优雅关闭时，先停止接受新请求，停止新的 WAL 提交，等待已提交组完成 `fsync`，再在关闭超时内等待 materializer 排空已提交队列并 checkpoint。
- 关闭超时后保留 WAL，由下次启动回放；不能为了快速关闭删除未物化段。
- materializer worker 必须被监督。worker 意外退出时健康状态进入 `degraded` 或 `blocked`，WAL 仍可写入直到容量阈值；不能静默停止消费。
- journal 进入 unhealthy 后，请求返回 503，health 暴露具体原因；恢复只能通过重启恢复或显式修复，不在运行中自动绕过。

## 故障语义

| 故障 | 行为 |
| --- | --- |
| 进程崩溃或优雅重启 | 成功返回前已提交的 WAL 回放；已确认请求不丢 |
| 掉电或宿主机崩溃 | 只有成功 `fsync` 的 commit 帧保证保留；未确认组的末尾帧丢弃 |
| `fsync` 失败或超时 | 失败组返回提交错误，客户端收到 503；journal unhealthy，停止接受新提交 |
| 磁盘容量不足 | 预检失败时在请求前返回 503；运行中达到阈值时拒绝新提交，不删除未物化记录 |
| 数据库不可用 | WAL 继续保留；物化重试；依赖数据库读的请求按现有错误路径失败 |
| rollout 目录不可写 | WAL 保留；物化重试；健康状态降级并告警 |
| 物化延迟超过 SLO | 健康状态 `degraded`；活动、统计和会话正文可能短时滞后，WAL 继续保留 |
| 身份已过期但仍存在 pending WAL | 清理延后或排除该身份，不能删除尚未物化的记录和 rollout |
| WAL 中间损坏 | 进入 `blocked`，拒绝新请求，保留现场 |
| WAL 尾部损坏 | 截断到最后有效 commit，继续启动 |
| 流式过程中进程终止 | 只有 `TurnStarted` 的活动在恢复时标 `partial`；客户端看到流中断 |
| response state 提交失败 | 不发送成功完成事件；返回 503 或流错误，避免下一轮出现无法续接的成功响应 |
| 克隆目录共享 node-id 或恢复身份冲突 | 阻断启动，要求隔离旧 WAL 或生成新 node-id |
| 多副本单节点永久丢失 | 该节点未物化的本地 WAL 无法由其他副本恢复；属于本期部署边界 |

## 容量与背压

- WAL 增长由物化延迟决定。监控未应用字节、未应用记录数、段数量和磁盘剩余空间。
- 启动后预检 WAL 目录可写和最小剩余空间；低于 `RECORDING_JOURNAL_MIN_FREE_BYTES` 时拒绝新请求。
- 未应用字节超过 `RECORDING_JOURNAL_MAX_PENDING_BYTES` 时拒绝新请求，避免 WAL 无限增长。
- 物化失败时按指数退避重试，不丢弃记录。达到容量阈值后 fail closed，不能用删除未物化段换取可用性。
- 已应用段在 checkpoint 持久化后删除；删除失败只告警，不影响正确性。
- 保留最近一个完整段用于诊断，是否保留由部署配置决定，不能影响回放。

## 可观测性

活动列表和 `/api/stats/*` 继续读取已物化的 PostgreSQL 数据，线程内容继续读取已物化的 JSONL。物化延迟在 `RECORDING_MATERIALIZE_LAG_SLO_SECONDS` 内属于允许的最终一致；超过 SLO 时健康状态进入 `degraded`，并在指标和日志中给出 pending 数量和最老 pending 记录时间。不得在读取接口中伪造尚未物化的活动或正文。

新增指标：

- `recording_journal_append_seconds`
- `recording_journal_commit_seconds`
- `recording_journal_pending_bytes`
- `recording_journal_pending_records`
- `recording_journal_durable_sequence`
- `recording_materialize_seconds`
- `recording_materialize_lag_records`
- `recording_materialize_lag_seconds`
- `recording_materialize_lag_slo_seconds`
- `recording_replay_records`
- `recording_commit_failures_total`
- `recording_commit_timeout_total`
- `recording_partial_total`
- `recording_wal_bytes_total`
- `recording_rollout_bytes_total`
- `recording_write_amplification_ratio`
- `recording_health`

请求指标：

- `first_token_ms` 在首块真正交给下游发送时计算，不再在 `start_stream` 前计算。
- `latency_ms` 在终止事件和 WAL 提交完成后计算，包含记录提交等待。
- 单独记录 `recording_commit_ms` 和 `materialize_ms`，区分请求等待和后台成本。

日志只包含 `activity_id`、`record_id`、序号、状态、错误码和计数，不包含提示词、模型输出、Provider Key、Endpoint Token 或 response state 正文。

健康状态至少区分：

- `ready`：WAL 可写、恢复完成、materialization lag 在 SLO 内。
- `degraded`：WAL 可写，但物化 lag 超 SLO、数据库/rollout 暂时不可用或 worker 重启。
- `blocked`：WAL 损坏、node-id 冲突、加密不可用或 journal unhealthy，拒绝新请求。

`recording_materialize_lag_slo_seconds` 默认 5 秒，作为活动/统计/会话可见性的运维 SLO；超过 SLO 必须告警，不能把最终一致当成无限期滞后。

## 配置

### 配置归属

本文列出的 `RECORDING_*` 是服务端进程级部署配置，沿用现有 `.env`、进程环境和容器环境注入方式，在启动阶段一次性解析为 `RecordingJournalConfig`。它们不是客户端设置、数据库配置或管理 API 字段，请求路径不读取环境变量。

- 目录、容量和提交参数属于部署配置；非法值必须在启动时 fail fast，不能回退到猜测值。
- `node-id`、`epoch`、checkpoint 和 WAL 段是运行时文件状态，不是环境变量。
- `RECORDING_PIPELINE` 只作为迁移期回滚开关；收口批次删除它和旧记录路径。
- 单元测试直接构造 `RecordingJournalConfig`，不依赖进程环境。

| 配置 | 默认值 | 来源 | 说明 |
| --- | --- | --- | --- |
| `RECORDING_JOURNAL_DIR` | `data/recording` | 环境变量 | WAL、checkpoint 和节点标识目录；必须在持久卷 |
| `RECORDING_JOURNAL_SEGMENT_BYTES` | `67108864` | 环境变量 | 单段轮转大小 |
| `RECORDING_JOURNAL_CHUNK_BYTES` | `1048576` | 内部默认 | 单个 data 分片的 payload 上限 |
| `RECORDING_JOURNAL_GROUP_COMMIT_MS` | `2` | 环境变量 | 组提交最大等待时间 |
| `RECORDING_JOURNAL_GROUP_COMMIT_MAX_RECORDS` | `64` | 内部默认 | 单组最大逻辑记录数 |
| `RECORDING_JOURNAL_GROUP_COMMIT_MAX_BYTES` | `8388608` | 内部默认 | 单组最大提交字节；单个大记录可以单独超过该值 |
| `RECORDING_JOURNAL_COMMIT_TIMEOUT_MS` | `5000` | 环境变量 | 组提交 `fsync` 超时；超时后 journal unhealthy |
| `RECORDING_JOURNAL_MIN_FREE_BYTES` | `1073741824` | 环境变量 | 低于该值拒绝新请求 |
| `RECORDING_JOURNAL_MAX_PENDING_BYTES` | `8589934592` | 环境变量 | 未应用 WAL 的硬上限；超过后拒绝新请求 |
| `RECORDING_RESPONSE_STATE_CACHE_MAX_ENTRIES` | `10000` | 内部默认 | pending response state locator 上限；超过后拒绝新请求 |
| `RECORDING_MATERIALIZE_LAG_SLO_SECONDS` | `5` | 环境变量 | 活动/统计/会话可见性 SLO；超过进入 degraded |

环境变量只承载部署和运行策略需要的值；分片大小、单组条数、单组字节数和 locator 上限保持 `RecordingJournalConfig` 的内部默认，测试和故障注入可以直接构造配置覆盖。

提交必须在 `fsync` 成功后对等待者返回，不提供“跳过 fsync”的运行模式。迁移期如需要回滚，使用短期 `RECORDING_PIPELINE` 开关选择旧路径；正式结构落地后删除该开关和旧路径。

## 迁移与删除

结构变化与行为变化分开验证。

### 基线测量

- 增加 `storage_ms`、`wal_append_ms`、`wal_fsync_ms`、`materialize_ms` 计时。
- 记录当前非流式和流式的首字节、末字节、活动写入和 response state 写入耗时。
- 建立压测基线；不在这一阶段改变行为。
- 用代表性小/大 payload、流式长输出、低/高并发分别采样，计算 WAL 预计字节、JSONL 物化字节和写放大比例。
- 设置 go/no-go：`storage_ms` 不是 p99 显著组成部分，或 WAL 写放大导致磁盘吞吐成为新瓶颈时，停止完整实现，回到更轻的持久化方案；不能因为方案已经设计完成就继续实现。

### 日志与物化实现

- 实现 `RecordingJournal`、`GroupCommitter`、`RecordingMaterializer`、`RecordingRecovery`。
- 使用临时测试入口在测试环境验证，不接入生产请求路径。
- 建立帧、加密、分组、checkpoint、尾部截断、幂等回放的单元和集成测试。

### 非流式切换

- 非流式处理器改为生成 `TurnCompleted`，只等待 WAL 提交。
- 删除处理器中的 `save_response_state` 和 `record_completed` 直接调用，`remember_protocol_model_provider` 改为 `ProviderObservation`。
- 保留短期回滚开关；验证通过后删除旧路径。

### 流式切换

- 流式处理器改为 `TurnStarted` 和 `TurnFinished` 提交边界。
- 首块等待 `TurnStarted` 提交，终止完成事件等待 `TurnFinished` 提交。
- 删除 `begin_stream`、`start_stream`、`complete_stream` 的请求路径直接调用。
- 删除流式路径中的 `remember_protocol_model_provider` 写数据库，统一由 `ProviderObservation` 物化。

### 收口

- 删除 `RECORDING_PIPELINE` 开关和旧同步记录分支。
- 确认处理器不再直接写 `Storage`、`ActivityStore` 或 `RolloutWriter`。
- 保留存储适配器供物化器和查询使用，不保留第二套请求记录链路。
- 更新部署文档中的持久卷、权限、容量和主密钥轮换要求。

迁移期禁止长期双写。若为了对比在生产短暂双写，必须有明确的结束日期、指标验证和删除批次，不能把双写作为回滚结构保留。

## 验证

### 单元与集成

- 帧编码/解码、分片重组、CRC、AES-GCM 认证和 AAD 绑定。
- 组提交只释放成功 `fsync` 的等待者；失败组全部失败。
- checkpoint 原子替换、落后 checkpoint、段轮转和清理。
- 非流式成功/失败、流式成功/失败/断开/超时。
- response state upsert 和下一轮 `previous_response_id` 读取。
- 同一线程并发活动、不同线程并行物化、重复回放不重复追加。
- 只有 `TurnStarted` 的活动恢复为 `partial`。
- 延迟物化后活动 `created_at`、rollout 索引时间和 JSONL `meta`/`turn`/事件/`turn_end` 时间保持原始请求时间。
- 已提交 `TurnFinished` 的活动不会被既有 `recover_interrupted` 提前降级为 `partial`。
- 物化 lag 超过 SLO 时健康状态进入 `degraded`，活动/统计/会话读取行为符合约定。

### 故障注入

- 在 WAL append 后、commit 前、commit 后、数据库占位后、线程追加后、数据库终态后分别终止进程。
- 校验重启后已确认请求存在；未确认请求不返回成功，但服务端可能保留已提交而客户端未收到的事实记录。活动/rollout/response state 不重复。
- 模拟 WAL 尾部截断、中间 CRC 损坏、加密认证失败、checkpoint 损坏。
- 模拟 WAL 目录不可写、磁盘低于阈值、`fsync` 失败、数据库不可用、rollout 目录不可写。
- 模拟 `fsync` 超时、物化 worker 退出、身份清理与 pending WAL 竞争、克隆目录 node-id 冲突。
- 校验健康状态、错误码和拒绝新请求行为。

### 性能

- 对比改造前/后的首字节、总延迟、p50/p95/p99、吞吐和 WAL 积压。
- 区分请求等待、WAL 提交和后台物化耗时。
- 记录 WAL 字节、rollout 字节和写放大比例，确认大 payload 与低并发场景没有把瓶颈转移到 WAL 写入。
- 验证首字节不再包含数据库插入和线程索引读取。
- 验证流式稳态不按 chunk 落盘，尾延迟包含 WAL 提交但不包含物化。

### 安全

- WAL 文件无明文提示词、模型输出、response state 或密钥。
- 日志、错误和指标不包含 payload。
- WAL 文件和目录权限符合数据目录要求。
- 主密钥轮换前排空 WAL；无法解密时阻断启动。

## 验收

- 已向客户端确认成功的请求，在进程崩溃、重启和掉电模拟后不丢记录。
- 掉电只可能丢失未确认的组提交尾部，不产生成功记录丢失或半条成功记录。
- 流式首块不再等待数据库插入；终止完成事件只在 `TurnFinished` 提交后发送。
- 同一活动重放不重复生成 activity、rollout、统计或 response state。
- 中断流统一标记 `partial`，不会伪装成成功。
- 延迟物化不改变活动、统计、线程 JSONL 和审计事件的原始时间语义。
- 已提交 finish 的活动不会被旧 `recover_interrupted` 错误降级；pending WAL 身份不会被清理器删除。
- `data/recording` 持久卷、权限、环境变量和备份说明已进入部署配置，而不是只写在设计文档。
- materialization lag 有 SLO、告警和 degraded 健康状态；活动/统计/会话可见性行为已明确。
- 写放大和首字节/尾延迟经过真实 payload 与并发压测，未把瓶颈从请求路径转移到 WAL 磁盘写入。
- `/v1` 协议、接入点解析、故障迁移、统计口径和会话内容格式保持不变。
- 请求路径不再直接调用同步记录写接口；旧路径和迁移开关在收口批次删除。

## 部署边界

- `RECORDING_JOURNAL_DIR` 和 `ACTIVITY_ROLLOUT_DIR` 必须位于持久卷；容器重建不能丢失 WAL。
- `deploy/docker-compose.yml` 同时挂载 `./data/sessions` 和 `./data/recording`；`deploy/.env.example` 与 README 已包含 `RECORDING_JOURNAL_DIR` 和容量、提交、物化配置，容器 UID 1000 必须对两个目录可写。
- 单实例数据面独占 WAL 目录写锁。多副本必须共享日志或使用数据库 outbox 后端，否则节点永久丢失时未物化记录无法由其他副本恢复。
- 多副本还需要粘性路由或共享 response state；否则下一轮请求落到没有 pending cache、数据库尚未物化的副本时会找不到 `previous_response_id`。
- 备份必须覆盖 WAL 与数据库、rollout 的一致状态；推荐在 checkpoint 完成后备份，或同时备份未应用 WAL。
- `ENCRYPTION_KEY` 轮换必须先排空 WAL，并遵循现有 Provider Key 和 response state 轮换流程。
- WAL、checkpoint 和 rollout 的磁盘容量、IOPS、文件系统语义属于部署验收项，不能用本地开发盘结果替代生产验证。

## 发布验证边界

代码已按本文结构接入生产请求路径。发布前仍需在真实 PostgreSQL、持久卷、进程终止、掉电模拟和并发流式压测环境执行部署验收；本地 mock、编译、lint 和定向单测证明的是代码路径，不等同于生产磁盘、数据库和网络条件。

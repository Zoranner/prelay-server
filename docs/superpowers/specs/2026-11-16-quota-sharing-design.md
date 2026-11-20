# 供应商额度与共享设计

> 本文是早期共享额度设计记录，当前协议与展示设计见 `2026-11-17-quota-design.md`。

## 目标

在供应商列表的“额度”列中紧凑展示三类剩余额度，并让共享按额度工作：

- 套餐额度：5 小时、周、月窗口，来自供应商自动适配器。
- 余额：供应商账户剩余金额，来自供应商自动适配器。
- 共享额度：被共享者使用拥有者额度，或使用拥有者设置的日/周/月限额。

不新增独立额度页面；共享设置与额度查看分开。

## 已确认规则

- 额度是供应商列表中的一列，替换原“可见范围”列。
- 共享记录按 `Provider + 被共享人 + 可选额度规则` 保存。
- 额度规则为空表示直接共享：被共享者看到拥有者的套餐或余额额度。
- 额度规则非空表示限额共享：被共享者只看到自己的额度。
- 限额规则支持日、周、月窗口自由组合，每个窗口独立限额、独立累计开关、独立清零边界。
- 实际可用额度取所有启用窗口剩余的最小值。
- 累计开启时，未用完额度滚到下一个同类周期，最多滚一个周期。
- 默认只启用一个窗口；额度列显示最紧窗口，浮层展开全部窗口。
- 创建者查看被共享人用量暂不实现。
- 套餐和余额不允许在客户端手填数值；取不到接口时显示“未接入”。

## 数据模型

### 共享额度

`identity_provider_shares` 增加 `quota_json TEXT`：

```json
[
  {
    "window": "day",
    "limit": 100000000,
    "accumulate": true,
    "reset_time": "00:00",
    "reset_weekday": null,
    "reset_day": null
  }
]
```

- `quota_json` 为 `NULL`：直接共享。
- `quota_json` 为非空数组：限额共享。
- `window` 只允许 `day`、`week`、`month`。
- 周末重置用 `week` 窗口和 `reset_weekday` 表达；周内累计、周日结束清零用日窗口 `accumulate=true`，周窗口 `reset_weekday=1, reset_time=00:00` 表达。

### 套餐与余额

不新增可手填字段。额度快照由服务端按供应商类型调用适配器：

- `deepseek`：`GET https://api.deepseek.com/user/balance`。
- `minimax`：`GET https://www.minimax.cn/v1/token_plan/remains`。
- 其他供应商暂返回 `unsupported`。

适配器结果按供应商缓存 60 秒，避免列表刷新反复请求上游。

## 协议

新增：

- `QuotaWindowKind`：`five_hour`、`day`、`week`、`month`。
- `ShareQuotaWindow`：共享额度窗口规则。
- `ProviderSharingUserInput`、`ProviderSharingUserResponse`：共享用户及其额度规则。
- `ProviderQuotaMode`：`own`、`shared_unlimited`、`shared_limited`。
- `ProviderQuota`：`plan`、`balance`、`shared`、`unsupported`、`error`。
- `QuotaWindowResponse`：窗口剩余、额度、已用、累计、重置时间。
- `ProviderQuotaResponse`：列表额度列的数据。

共享接口改为：

- `GET /api/providers/:provider_id/sharing` 返回 `visibility` 和 `users`，仅创建人可读。
- `PATCH /api/providers/:provider_id/sharing` 接收 `visibility` 和 `users` 全量替换。
- `GET /api/providers/quotas` 返回当前身份可见供应商的额度快照。

## 额度计算

### 套餐

- 5 小时窗口来自适配器返回的当前滚动窗口剩余百分比和重置时间。
- 周、月窗口来自适配器返回的剩余百分比和重置时间。
- 适配器只提供百分比时，`remaining`、`limit`、`used` 为空，界面显示百分比。

### 余额

- 显示币种和剩余金额。
- `available=false` 或余额为零时显示警告状态。

### 共享限额

- 按 `provider_id + grantee_identity_id` 聚合 `identity_activities` 的输入与输出 Token。
- 周期边界按时区 `+08:00` 计算；日窗口默认 00:00，周窗口默认周一 00:00，月窗口默认 1 日 00:00。
- 累计开启时，上一周期剩余额度加入当前周期，最多加入一个周期额度。
- 多个窗口同时启用时，任意窗口剩余为零即不可用。

## 界面

### 供应商列表

- 新增“额度”列，宽约 140。
- 套餐：24px 三同心圆环，中心显示最紧窗口剩余百分比；浮层显示三个窗口。
- 余额：显示 `¥128.40` 或 `$128.40`；浮层显示更新时间和可用状态。
- 共享限额：显示最紧窗口剩余百分比和紧凑进度条；浮层显示全部窗口。
- 直接共享：沿用拥有者的套餐圆环或余额显示。
- 未接入：灰色“未接入”；查询失败：灰色“查询失败”。

### 共享抽屉

- 只配置共享，不显示额度统计或用量。
- 每个被共享人一行：身份、模式（不限额/限额）、编辑、移除。
- 限额模式默认只启用日窗口；可增加周、月窗口。
- 每个窗口配置额度、是否累计、清零时间；周窗口额外配置星期，月窗口额外配置日期。

## 边界

- 不实现“创建者查看被共享人用量”。
- 不实现独立额度页面。
- 不实现套餐/余额手填。
- 不实现跨服务实例共享。
- 共享限额在请求解析时按已记录用量检查；任一窗口剩余为零时跳过该 Provider。并发请求的原子预留和严格超额拒绝在后续补充。

## 验证

- 协议 DTO 序列化与反序列化。
- 共享规则校验：重复身份、拥有者、未知身份、空规则、非法窗口。
- 共享额度周期计算：日/周/月边界、累计、上一周期、多个窗口取最小值。
- DeepSeek 余额解析、MiniMax 额度解析。
- 共享接口全量替换和删除。
- 共享限额耗尽后不再解析该 Provider。
- 客户端额度列三种形态、共享抽屉限额编辑、空态和错误态。

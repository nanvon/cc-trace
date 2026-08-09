# ADR-0026：OpenAI／Anthropic 公开 Statuspage 状态链进入首版

- 状态：**已确认**
- 日期：2026-08-09
- 确认依据：产品所有者明确要求实现 Codex 与 Claude Code 官方服务状态的显示，并确认按 cc-bar
  行为加 `showServiceStatus` 开关（默认开）；首版只在紧凑面板显示，主窗口不显示。
- 相关文档：[产品范围](../产品范围.md)、[额度领域模型](../额度领域模型.md)、[技术架构](../技术架构.md)、
  [信息架构与核心流程](../信息架构与核心流程.md)、[设计方向与状态规范](../设计方向与状态规范.md)、
  [文案与国际化](../文案与国际化.md)、[测试策略](../测试策略.md)

## 背景

首版范围原本把「OpenAI、Anthropic 公开 Statuspage 状态链」列入“明确不做”
（[产品范围](../产品范围.md)「明确不做」与[额度领域模型](../额度领域模型.md) 第 3.3 节），
理由是该能力在旧版 cc-bar 中已存在，但 CC Trace 首版没有为它定义数据边界与验收标准。

产品所有者决定把它纳入首版，并要求行为对齐 cc-bar：在紧凑面板每个 Provider 卡头部显示
官方状态页圆点，提供设置开关。这条状态链与 Provider 额度 API 的本地可达状态是**两条独立
状态链**——Statuspage 报告的是 OpenAI／Anthropic 官方服务的公开故障，额度三维状态报告的
是“本机凭据与请求是否成功”，两者不得合并（该边界在额度领域模型第 3.3 节本就保留）。

## 决策

### 1. 数据源与模型

- 数据源为两个公开 Statuspage.io 接口，无凭据：
  - OpenAI：`https://status.openai.com/api/v2/status.json`
  - Anthropic：`https://status.claude.com/api/v2/status.json`
- 模型：`indicator`（`none`／`minor`／`major`／`critical`／`maintenance`／`unknown`）、
  `description`、`page.updated_at`、本地抓取时刻 `fetchedAt`。
- 解析失败或枚举值无法识别一律归为 `unknown`，不视为错误状态。
- 请求复用 `providers::http` 共享客户端（含系统代理探测），单次请求超时 15 秒，服从
  [技术架构](../技术架构.md) 参数基线。

### 2. 与额度状态链的关系

- 服务状态**不进**三维状态模型（活动／新鲜度／可用性），不参与 `ProviderAvailability`
  的取值，不影响刷新调度、退避与身份变化。
- 服务状态**不进** Overall Signal，不参与总体状态点与 tooltip 的 explainables。
- 契约与事件独立：新增 `service-status://updated`，不并入 `quota://updated`。
- 两个 Provider 的服务状态互相独立，一个失败不影响另一个。

### 3. 调度与存储

- Scheduler 独立循环，固定 5 分钟一次（Statuspage 变化很慢，不跟额度刷新间隔抖动），
  启动后立即拉一次；用户手动整体刷新时附带拉一次（与 cc-bar 一致）。
- 抓取失败保留上一份内存值，不清空；`unknown` 由前端决定不显示圆点。
- **无磁盘缓存**：数据是公开信息、可随时重建，失败也不影响任何既有数据文件。

### 4. 展示

- 只在**紧凑面板**的 Provider lane 头部显示 6px 圆点（与 cc-bar popover 一致）；
  主窗口与系统区域不显示。
- 圆点颜色：`none` 绿、`minor` 黄、`major` 橙、`critical` 红、`maintenance` 蓝；
  `unknown` 不画点。颜色映射到现有语义色 token，不新增色值。
- 圆点 tooltip 显示 `description`（缺失时用 indicator 标签）与「N 前更新」。
- 隐私模式只隐藏账号标识，不影响服务状态点。

### 5. 设置开关

- 新增设置项 `showServiceStatus`，默认 `true`，位于设置视图「通用」组
  （与 cc-bar 的 Service status dot 语义一致）。
- 开关只控制圆点是否绘制；后台 5 分钟拉取不受开关影响（与 cc-bar 一致）。

## 与 cc-bar 的差异

- cc-bar 的 ServiceStatusClient 使用独立 10 秒超时；CC Trace 复用共享 HTTP 客户端与
  参数基线（15 秒超时），不做第二套超时取值。
- cc-bar 在主窗口不显示服务状态点，CC Trace 相同。
- 其余行为（调度间隔、失败保留、unknown 不显示、开关语义）与 cc-bar 一致。

## 代价与风险

- `status.openai.com`／`status.claude.com` 在国内网络可能不可达；失败时圆点不显示，
  不影响任何额度与用量功能。这是可接受的降级，与 cc-bar 行为一致。
- Statuspage 的 `indicator` 粒度是官方公开信息，可能出现「额度接口正常但状态页标记
  minor」的错位——这是公开状态页的固有属性，两条状态链不合并正是为了容纳这种错位。
- 新增一次独立轮询循环与一个前端订阅事件，改动面可控，不触碰 Provider 契约与调度契约。

## 复审条件

- 若 Statuspage 端点变更协议或不可用持续一个发布周期，评估移除圆点或改用其他公开来源。
- 若主窗口需要承载服务状态，需另行决策，不自动扩展本 ADR 的展示范围。

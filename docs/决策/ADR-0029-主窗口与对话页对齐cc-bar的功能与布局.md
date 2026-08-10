# ADR-0029：主窗口与对话页对齐 cc-bar 的功能与布局

- 状态：已确认
- 日期：2026-08-10
- 相关文档：[ADR-0020](ADR-0020-主窗口改为本地用量页.md)、[ADR-0024](ADR-0024-主窗口分组侧边栏导航.md)、[ADR-0028](ADR-0028-全部表面改为贴合式弱化卡片层级.md)、[产品范围](../产品范围.md)、[信息架构与核心流程](../信息架构与核心流程.md)

## 背景

主窗口与对话页按 cc-bar 对比后，产品所有者确认「功能、内容、需求、布局」全面对齐 cc-bar 的对应形态。逐项对照后，多数差异在 [ADR-0020](ADR-0020-主窗口改为本地用量页.md)／[ADR-0024](ADR-0024-主窗口分组侧边栏导航.md)／[ADR-0028](ADR-0028-全部表面改为贴合式弱化卡片层级.md) 与 `docs/产品范围.md` 中已有决策，本次统一推翻或补全：

1. 用量页顶栏：cc-bar 的 range 分段＋扫描状态＋手动刷新在右上；CC Trace 的筛选器在内容区、无手动刷新（[产品范围](../产品范围.md)「紧凑入口……不提供手动扫描按钮」）。
2. KPI 无 delta：ADR-0028 第 37 行明确「不做 delta 对比」。
3. 对话页为列表＋整页跳转详情（路由），cc-bar 是左列表右详情分栏。
4. 对话列表无时间范围过滤（恒查全量）、无项目筛选菜单；行内无模型与速度徽标。
5. 用量页无 Token 拆分面板与 Fast 汇总（Provider 卡注释明确「Fast 等效 Token 与倍率不上卡」）。
6. 对话详情缺对话 ID（可复制）、git 分支。
7. Codex／Claude 对话标题恒为「未命名对话」：Rust 扫描从未读取 `~/.codex/session_index.jsonl` 与 `~/.claude/history.jsonl`，也未解析 Codex `user_message` 事件与 Claude `user` 行兜底（对齐 cc-bar `ConversationTitleIndex`）。

## 决策

### 1. 对话标题来源（对齐 cc-bar `ConversationTitleIndex`）

- Codex：`~/.codex/session_index.jsonl` 的 `id → thread_name` 优先；JSONL 内 `event_msg.user_message` 的 `payload.message` 首条纯文本兜底。
- Claude Code：`~/.claude/history.jsonl` 的 `sessionId → display` 优先；JSONL 内非 sidechain `user` 行的 `message.content` 首条纯文本兜底。
- 标题统一清理：空白折叠、去掉 `<` 前缀、截 80 字符。
- 索引文件缺失或损坏时按兜底处理，不升级为扫描失败；标题正文只进 `conversations` 表，不落日志。
- `CodexCursor`／`ClaudeCursor` 增加 `pending_title`（`#[serde(default)]` 兼容旧游标），Codex 另增 `source_id` 与 `project_hint`。游标约束修订见 [数据存储与用量索引](../数据存储与用量索引.md) §4.1。
- 存量数据：已扫描文件在 mtime／size 未变时不重扫，标题修复后需在设置页「重新计算用量」一次补齐。

### 2. 用量页顶栏与手动刷新

- 顶栏改为：左侧页面标题，右侧 range 分段＋扫描状态＋手动刷新按钮（对齐 cc-bar topBar）。
- 自定义日期范围输入行仅在 `custom` 档显示（对齐 cc-bar customRangeRow）。
- 手动刷新按钮调用既有 `usage_scan_start`，扫描中禁用并轮询状态，完成后重载当前范围；推翻 [产品范围](../产品范围.md)「不提供手动扫描按钮」对主窗口的限制（紧凑入口仍无手动扫描按钮）。

### 3. KPI delta 对比

- 每个 KPI 卡增加与前一个等长区间的环比（↑↓％），红涨绿跌、`tabular-nums`；无数据或上一期为零时不显示。
- 推翻 [ADR-0028](ADR-0028-全部表面改为贴合式弱化卡片层级.md) 第 37 行「不做 delta 对比」。`all` 与 `custom` 档无有意义的等长前区间，不显示 delta（对齐 cc-bar `previousBounds`）。

### 4. Token 拆分面板与 Fast 汇总

- KPI 行下方新增 Token 拆分面板：总 Token hero 数字＋输入／输出／缓存命中堆叠条＋命中率（对齐 cc-bar `TokenBreakdownView`）。
- Fast 用量（`rawTokens > 0` 时）在同面板内展示：Fast Tokens／计费等效 Tokens／倍率（混合模型显示最小–最大）／Fast 占比。Fast 费用不展示：当前汇总契约没有按速度拆分的费用，不得拼造。

### 5. 对话页分栏

- Conversations 视图改为左列表右详情分栏（对齐 cc-bar F-17 分栏形态）；点击行在右侧展示详情，不再整页跳转。
- 删除 `conversation-detail` 子路由与 `ConversationDetailView.vue`，详情抽为 `ConversationDetailPane.vue` 纯组件；列表无选中项时右侧显示引导文案。
- 窄容器（内容区 < 860px）回落单列堆叠。
- 列表行补模型名（`models` 去重列表）与速度徽标（Fast／Mixed，对齐 cc-bar `UsageSpeedBadge`）。

### 6. 对话页时间范围与项目筛选

- 对话列表与项目菜单的过滤时间范围与用量页共享（`dashboardRange` 全局状态），范围变化时列表跟随刷新。
- 新增项目筛选菜单：全部项目＋按脱敏项目名分组（`usage_list_conversation_projects`，最近活动排序）；菜单与列表共用同一过滤范围。

### 7. 对话详情补全

- 详情展示对话 ID（`source_id`，会话 UUID 非账号明文）并可复制；展示 git 分支（Claude JSONL `gitBranch`）。
- Standard／Fast 费用拆分沿用既有速度档位表（原始 Token／计费等效／费用）。

### 8. Rust 契约与 schema

- `UsageConversation` 增加 `source_id`、`branch`、`models`（会话去重模型列表，不受查询过滤影响）。
- `conversations` 表新增 `source_id`、`branch` 列，schema 升至 v5（`ALTER TABLE` 迁移，旧数据不重扫）。
- 新增 `usage_list_conversation_projects` command。
- Codex 会话补充 `project_hint`（`session_meta.cwd` 末段），此前 Codex 对话恒无项目提示。

## 与 cc-bar 的差异（不照搬的部分）

- 分页保留每页 20 条的既有分页控件，不改为「显示更多」无限滚动（交互等价，范围克制）。
- 项目菜单不做「已不存在／未分配／系统任务」分组：CC Trace 首版只有脱敏项目提示，没有路径与系统任务语义。
- Fast 汇总不展示费用：汇总契约无按速度拆分的费用（对话详情页的速度档位表仍展示费用）。
- 视觉仍按 [ADR-0028](ADR-0028-全部表面改为贴合式弱化卡片层级.md) 的贴合式弱化卡片，不引入毛玻璃与 Apple 系统色。

## 理由

- 标题是对话列表的第一识别字段，全部「未命名对话」让列表失去检索价值；cc-bar 的索引＋兜底两级方案已被实机验证，直接对齐成本最低。
- 顶栏、delta、Token 拆分与 Fast 汇总是 cc-bar 被认可的信息层级核心（大数字锚点、环比决策价值），缺项让主窗口「信息平铺」。
- 分栏详情消除列表↔详情整页切换的上下文断裂，cc-bar 的交互形态即验收基线。
- 时间范围与项目筛选让「按项目复盘」成为可完成的任务，Rust 查询参数已存在，只补项目列表接口。

## 后果

- Rust：`parser.rs`（标题索引、user 行、source_id／branch／Codex project_hint）、`title_index.rs`（新模块）、`model.rs`（cursor 与 ConversationFact 扩展）、`contracts/usage.rs`、`storage/usage_db.rs`（schema v5、查询列、项目列表）、`commands/usage.rs`、`lib.rs`（新 command）。
- 前端：`MainView.vue`（顶栏／delta／Token 拆分／Fast）、`ConversationsView.vue`（分栏／时间范围／项目筛选／行内容）、`ConversationDetailPane.vue`（新组件）、`store.ts`（`dashboardPrevious`、`startScan`）、`ranges.ts`（`usagePreviousRange`）、i18n 中英同步。
- 文档：[产品范围](../产品范围.md)（主窗口段、Conversations 段、首版简化与验收）、[数据存储与用量索引](../数据存储与用量索引.md)（v5 列、标题来源、游标约束）、[ADR-0028](ADR-0028-全部表面改为贴合式弱化卡片层级.md) 修订注记、执行清单与 [AGENTS.md](../../AGENTS.md) 阶段状态。
- 浏览器预览与实机走查仍属第 13／16 阶段，未做之前不标记为已通过。

## 复审条件

- 若 Fast 汇总的倍率区间文案在窄容器换行，允许折行或省略号，不改语义。
- 若对话详情分栏在最小窗口（内容区 720px−176px）过于拥挤，允许左栏收窄到 360px 以下改为上下堆叠，不改交互语义。
- 若项目名含空格或特殊字符导致 select 选项显示异常，允许对名称做展示截断，不改变筛选语义。

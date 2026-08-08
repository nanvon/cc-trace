# OpenCode 数据源（用量）

> 状态：事实文档；OpenCode 用量功能已于 2026-08-08 按本文件进入首版实现（Rust SQLite 扫描＋测试，
> 见 [产品范围](产品范围.md)「本地用量与历史」）。
>
> 本文件拥有：**OpenCode 本地 SQLite 会话库的读取协议、表结构与字段语义、增量与去重规则、价格口径、会话元数据与时间规则**。
>
> 事实来源：cc-bar `Core/Usage/OpencodeScanner.swift` 及相关改动（2026-08-07 提交 `49ad45c`，统计页接入同日 `1023969`、`5949687`），叠加 `CCBarTests/OpencodeScannerTests.swift` 的 SQLite fixture 与断言。cc-bar 代码只作事实来源，不作需求；实现时以本文件为准，不需要再读 cc-bar 源码。

## 1. 数据源位置与文件格式

- 数据库：`~/.local/share/opencode/opencode.db`（SQLite，OpenCode Desktop 与 CLI 共用同一份）。
- 不是 JSONL：增量与去重协议与 Codex／Claude 的字节 offset 扫描完全不同（见 §4）。
- 表结构（测试 fixture 建的最小列集，实际库同名字段超集）：`project`、`workspace`、`session`、`message`、`part`。扫描实际用到的列：

| 表 | 用到的列 | 用途 |
|---|---|---|
| `message` | `id`、`session_id`、`time_created`、`data`（JSON） | 每行一条消息；`time_created` 是 Unix 毫秒 |
| `session` | `id`、`title`、`directory`、`workspace_id` | 会话元数据；`directory` 是项目路径来源 |
| `workspace` | `id`、`branch` | 分支名（LEFT JOIN，可为空） |
| `part` | `session_id`、`time_created`、`id`、`data`（JSON） | 仅用于空标题会话的标题兜底 |

- 扫描主查询：`message JOIN session ON session_id`，`LEFT JOIN workspace ON workspace_id`，`WHERE message.time_created >= watermark ORDER BY time_created, id`。
- 库缺失、打开失败或 SQL prepare 失败（表结构不符，OpenCode 未来版本可能迁移）时 **no-op**：返回原水位状态，不报错。

## 2. 计入规则与消息字段

`message.data` 是 JSON，角色语义：

| 角色 | 处理 |
|---|---|
| `user` | 不产生用量；读取嵌套 `model{providerID, modelID, variant}` 作为会话模型标签兜底，供后续 assistant 继承 |
| `assistant` | **计入**：读顶层 `providerID`／`modelID`、`tokens{...}`、`cost` |
| 其余 | 不参与 |

- 模型标签格式 `providerID/modelID`（如 `opencode-go/deepseek-v4-flash`），**variant 不拼入**；assistant 自身顶层字段优先，读不到时回退会话内最近一条 user 消息继承的模型，再兜底 `"unknown/unknown"`（避免增量扫描只追加 assistant 时标签丢失）。
- `tokens` 结构：`input`、`output`、`reasoning`、`cache{read, write}`、`total`；**`total = input + output + reasoning + cache{read, write}`，`output` 字段不含 `reasoning`**。
- **`reasoning` 并入 `output` 并单独记录 reasoning 明细**（总量与 cc-bar 一致：`output + reasoning`；与 pi 的口径一致）。
- 零费用跳过：`tokens.total <= 0` 且 `cost` 为 `nil` 或 `0` 的消息（工具调用收尾等）不产生用量。
- `cost` 是**美元小数**（Decimal），不是整数 nanos；经 String 中转避免 Double → Decimal 二进制浮点误差。

## 3. 增量扫描与去重协议

- **watermark**：`max(message.time_created)`（Unix 毫秒），写入 `ScanState` 的 `opencodeLastMessageTime`；查询 `>= watermark`，按 `(time_created, id)` 排序。
- **全局去重键**：`message.id`，跨会话共享（compaction 重写／时间戳回跳兜底），写入 `opencodeSeenMessageIds`。
- seen 集合上限 20000 条（超出保留最近 20000，与 Claude／Pi scanner 一致）。
- 被删除的会话**不回扫**（SQLite 库视为只增，与 JSONL append-only 假设一致）。
- **只读打开**（`SQLITE_OPEN_READONLY`）：与 OpenCode 运行中的写入通过 WAL 并发安全。

## 4. 价格口径

- **官方 `cost` 是费用总额真值**；分项用 `Pricing.costBreakdown` 补算，查不到分项时**官方总额降级计入 output**（保证对话页与统计页总额一致，且不误标「未定价」）。
- 分项补算**只用 cc-bar 本地静态表**：`PricingCatalogStore.rate` 对 `app == .opencode` 的 Standard 返回 nil、Fast 返回 nil——**不查在线价格目录**，缺价**不触发远端刷新**（`needsRemotePriceRefresh` 恒 false），也不参与价格目录 fingerprint 的远端部分。
- 本地静态表命中时四项拆分之和可能与官方 `cost` 不等：以官方 `cost` 为总额真值。
- `normalize` 会剥 `opencode-go/` 等 provider 前缀再查表，因此 OpenCode 里用到已收录模型（如 Claude 系）时能拿到分项。
- **无 Fast 档位概念**：speed 恒为 `standard`；`billingEquivalentMultiplier` 返回 nil。

## 5. 会话元数据

- 会话键：`opencode:<session id>`。
- 标题：`session.title` 非空用之；空标题用该会话 `part` 表按 `(time_created, id)` 排序的**第一条** part，若其 `type` 为 `text` 则取 `text` 字段兜底（非 text 即跳过，不继续找下一条）；经标题清理（空白折叠、去前缀、截断）。
- 项目：`session.directory` 经项目解析器解析为**脱敏**项目提示（cc-bar 用 `ConversationProjectResolver`，source 为 cwd）；不得保存明文路径。
- 分支：`workspace.branch`（可空）。
- 识别：cc-bar 的 `UsageApp` 增加 `.opencode` case，并有独立识别色 **OpencodeAccent**（浅 `#0F766E` / 深 `#2DD4BF`）。

## 6. 时间与归日

- 时间戳：`message.time_created`（Unix 毫秒）→ 日期。
- 归日按机器**本地日历日**（与 cc-trace 现有 `day_local` 规则一致）。

## 7. 定位与统计页接入

- **不是订阅服务、没有额度**：只进主窗口本地用量统计，不进额度轮询／菜单栏／Popover／悬浮窗／Timeline（与 Pi 定位一致）。
- cc-bar 接入范围（2026-08-07）：主窗口 Overview 的 KPI、图例、按 Provider 堆叠柱图、By service／By model、Conversations 列表与详情；`AppState.opencodeTodayCost` 今日费用；ScanCache 升 v11。
- cc-bar 统计页支持**按服务过滤**：设置页「统计服务」组，Codex／Claude Code／Pi／OpenCode 默认全开，关闭后 KPI／Token 拆分／每日用量／按服务／按模型／对话页统一过滤，占比分母只统计可见服务（归一化），全关时按服务面板显示空态引导。

## 8. cc-trace 实现时的映射与决策点

| 项 | cc-bar 行为 | cc-trace 规则 | 处置 |
|---|---|---|---|
| 六维 Token 映射 | input/output/reasoning+cache{read,write}（**output 不含 reasoning**，cc-bar 计为 `output + reasoning`） | uncached_input / output / reasoning_output / cache_read / cache_write_5m / cache_write_1h | input→uncached_input；output→**output + reasoning**，reasoning_output 单独记录明细（2026-08-08 修正，与 cc-bar 总量一致）；cache.write→cache_write_5m |
| 零费用跳过 | total<=0 且 cost 为 nil/0 跳过 | 无 total 一致性校验 | **已应用**（2026-08-08）：沿用 cc-bar 跳过规则 |
| 模型缺失 | 写字符串 `"unknown/unknown"` | 模型缺失写 `NULL`，禁止伪装 unknown | **已决策**：assistant 顶层 `providerID/modelID` → user 继承 → NULL |
| 缺 cost | `costUSD = nil`、breakdown=nil，总额不降级 | 未定价费用为 `NULL`，不得按 0 | 一致，无冲突 |
| 官方 cost 与分项不一致 | 以官方 cost 为总额真值，分项仅补算 | 费用总额与分项以 `api_equivalent_cost_nanos` 单一列表达 | **已决策**（2026-08-08）：官方 cost 存入 `api_equivalent_cost_nanos` 作为该来源的权威费用；`pricing_fingerprint = NULL`，重计价永不触碰，语义按「来源自带费用」理解，不复用 API 等值口径的 UI 文案 |
| 价格目录 | 分项只用本地静态表，不查远端、缺价不刷新、不参与 fingerprint | 不参与 fingerprint、缺价刷新、重计价 | **已实现**：完全不参与（cc-trace 未实现分项补算，官方 cost 即总额） |
| 来源标识 | `UsageApp.opencode` | `usage_entries`／`conversations` 需扩展 opencode 来源列 | **已实现**（2026-08-08，schema v4）：source CHECK 含 `'opencode'` |
| 扫描水位 | `ScanState` v11 的 `opencodeLastMessageTime` + `opencodeSeenMessageIds` | SQLite `scan_files` 水位表是 JSONL 文件级语义，不适用 | **已实现**：新增 `opencode_state` 表（`watermark_ms` + `seen_ids` JSON，上限 20000）；全局去重由 `usage_entries` 唯一索引兜底 |
| 对话元数据 | `ConversationSeed` key `opencode:<id>`，`includesSubtasks: false` | `conversations` 表 | **已实现**：会话键 `opencode:<session id>`；标题取 `session.title`，空时用该会话第一条 part 的 text 兜底；项目来自 `session.directory` 末段脱敏提示；branch 未存储（首版无分支展示） |
| Windows | cc-bar 仅 macOS，未验证 | `%USERPROFILE%\.local\share\opencode\opencode.db` 路径与 WAL 行为**未验证** | 待确认 |

## 9. 证据等级

| 事实 | 证据 |
|---|---|
| 库位置、表结构、字段语义、增量 watermark、message.id 去重、只读 WAL | cc-bar 代码（`49ad45c` `OpencodeScanner.swift`）+ `OpencodeScannerTests` fixture 断言 |
| reasoning 并入 output、零费用跳过、模型继承回退 | `OpencodeScanner.swift` + 测试断言 |
| 官方 cost 优先、未收录模型降级计入 output | `OpencodeScanner.swift` `makeEntry` |
| 分项只用本地静态表、不查远端、不参与 fingerprint | `Pricing.swift` 与 `PricingCatalogStore.swift`（`49ad45c`） |
| OpencodeAccent 色值 | cc-bar `OpencodeAccent.colorset/Contents.json` |
| 真实库的实际列超集与 OpenCode 版本行为 | 未在真机抽样，待实机验证 |
| Windows 路径与行为 | 未验证 |

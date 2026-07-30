# 07 · 测试现状与 TDD 行为基线

## 现状快照

`CCBarTests/` 当前只有 1 个 XCTest 文件、2 个脱敏 JSONL Fixture、25 个 `test...` 方法。**代码已确认**：`ccbar.xcodeproj` 有独立 `CCBarTests` target，scheme 也选择了它。本次审计没有运行测试、构建或模拟器/App；下文是“测试代码断言了什么”，不是“本次验证通过了什么”。

## 25 个测试方法逐项清单

| # | 测试方法 | 行为基线 |
|---:|---|---|
| 1 | `testCodexNormalResponseKeepsFiveHourPrimaryAndWeeklySecondary` | 正常 Codex primary 5H、secondary weekly、剩余百分比解析 |
| 2 | `testCodexTemporaryWeeklyOnlyResponseUsesWeeklyAsPrimary` | 只有 7 天窗口时以 weekly 为 primary，不伪造 5H |
| 3 | `testCodexUnknownWindowDoesNotPretendToBeFiveHour` | 未知秒数归 `.unknown` |
| 4 | `testClaudeMergesLegacyWindowsAndDynamicFableLimit` | Claude legacy 窗口与 dynamic session/weekly/model limit 合并 |
| 5 | `testMissingResetCarriesOnlyStillValidMatchingReset` | 只继承仍在未来且匹配的 reset；已过期不继承 |
| 6 | `testLegacyCodexCacheReclassifiesSevenDayFiveHourSlot` | 旧 cache 的 fiveHour 字段若 windowSeconds=weekly，解码后改成 weekly |
| 7 | `testMenuBarBothDoesNotDuplicateWeeklyOnlyPrimary` | 菜单栏 both 选择不会重复 weekly-only primary |
| 8 | `testHistoryResetsBaselineWhenPrimaryKindChanges` | quota history 主窗口 kind 变化时清 event、换 baseline |
| 9 | `testClaudeAssistantUsageMapsFastStandardAndMissingSpeed` | Claude speed 映射 fast/standard/unknown，保留 cache read/create |
| 10 | `testClaudeRepeatedStreamingLinesKeepSameMessageIdentity` | 同 message id 的流式行合并，完整行优先，半成品不算 complete |
| 11 | `testClaudeCacheCreationTTLParsingPreservesAggregate` | legacy/detailed/mismatched/clamped cache creation TTL 仍保留 aggregate |
| 12 | `testCodexServiceTierTransitionsAndUnknownValue` | Codex default/priority/default → standard/fast/standard，未知 → unknown |
| 13 | `testCodexThreadSettingsFixtureReadsNestedPriorityTier` | 从 nested thread settings 读取 model 和 priority/fast |
| 14 | `testCodexIncrementalStateKeepsFastTierAndCumulativeSignature` | ScanFileState Codable 保留 fast tier 和累计 signature |
| 15 | `testCodexTruncationResetsTierModelAndDuplicateGuard` | 截断清 offset/model/tier/signature，但保留 conversation id |
| 16 | `testCodexCumulativeUsageSignatureFiltersOnlyUnchangedTotals` | 相同累计总量过滤，任一 Token 改变则产生新 signature |
| 17 | `testFastPricingUsesExplicitTierRatesAndKeepsUnpricedAsNil` | Codex/Claude Fast 明确费率；超长上下文/未知模型/unknown speed 可为 nil |
| 18 | `testGPT55StandardLongContextProAndFastRates` | GPT-5.5 短/长上下文、Pro、Fast 价格切换 |
| 19 | `testClaudeCacheCreationTTLUsesSeparateStandardAndFastRates` | Claude Sonnet/Opus/Haiku 的 5m/1h cache 价格和历史模型价格 |
| 20 | `testUnknownPriceStillAggregatesAsZeroCost` | 未知价格 entry cost nil，但 UsageBucket Token 仍聚合、显示 cost 0 |
| 21 | `testFastEquivalentTokensAreSeparateFromRawTokens` | Fast 原始 Token 与 billing-equivalent Token 分开；倍率按模型计算 |
| 22 | `testFastMultiplierHidesKnownRangeWhenUnknownModelsAreMixedIn` | 已知/未知 Fast 模型混合时不显示误导倍率范围 |
| 23 | `testCodexFixtureScansTierTransitionsIncrementallyAndAfterTruncation` | Codex Fixture 全量、追加、无变化、截断后增量扫描与 rollup 对齐 |
| 24 | `testClaudeFixtureDefersPartialLineDeduplicatesFilesAndResumesByOffset` | Claude Fixture 半行延迟、跨文件 message 去重、追加后按 offset 恢复 |
| 25 | `testFastCacheSchemaVersionsAreUpgradedTogether` | Scan v9、Usage Rollup v8、Conversation Rollup v5 和 fingerprint 长度基线 |

## Fixture 清单

### `CCBarTests/Fixtures/codex-fast-scan.jsonl`

- 9 行脱敏 JSONL。
- 固定 conversation UUID、临时 cwd、session_meta、turn_context。
- `thread_settings_applied` 在 default / priority 之间变化。
- `token_count` 同时含 total_token_usage、last_token_usage；用于测试 cumulative signature 和增量请求。

### `CCBarTests/Fixtures/claude-fast-scan.jsonl`

- 4 行脱敏 assistant JSONL。
- 同一 session、临时 cwd、不同 message id。
- 包含 fast、standard、缺失 speed；包含 cache creation aggregate 和 5m/1h 细分；第一行没有 stop_reason，用于模拟 streaming 半成品。

**代码已确认**：Fixture 在测试内复制到临时目录后追加/截断，未读取真实用户日志。Fixture 内容没有真实凭据；新版若复用必须保留脱敏和临时目录原则。

## 当前覆盖矩阵

| 领域 | 当前测试 | 覆盖程度 | 静态审计判断 |
|---|---|---|---|
| Codex quota parser | 1–3 | 基本窗口/未知窗口 | 有；没有网络响应/HTTP 层 mock |
| Claude quota parser | 4 | legacy + dynamic 一条组合 | 有最小基线；未覆盖大量字段/错误 |
| reset 继承 | 5 | 未来/过期 | 有 |
| quota cache migration | 6 | 一种旧 seven-day 误命名 | 有最小基线 |
| menu bar selection | 7 | weekly-only 去重 | 有；没有 UI snapshot |
| quota history | 8 | kind 变化 | 有；没有 daily prune/文件损坏 |
| Claude scanner | 9–11、24 | speed、stream、TTL、offset、去重、partial | 较强；没有 20k seen cap、mtime 异常、权限拒绝 |
| Codex scanner | 12–16、23 | tier、state、signature、truncate、fixture incremental | 较强；没有 active/archive mtime 冲突和大量异常行 |
| Pricing | 17–22 | tier、long context、cache、unknown、Fast | 较强；没有远端目录 decoder/store 网络行为 |
| Rollup invariants | 23/24 helper | 每日与对话 Token/cost 一致 | 有核心不变量 |
| Schema versions | 25 | 3 个版本 + fingerprint 长度 | 有最小基线 |
| AppState | 无 | 0 | 缺少身份变化、fallback、backoff、snapshot retain、imported concurrency 测试 |
| Scheduler | 无 | 0 | 缺少 interval cancel/restart、首次触发和 cancellation 测试 |
| Credentials/Keychain | 无 | 0 | 只能静态阅读；不能当作真机授权已验证 |
| Antigravity | 无 | 0 | 缺少 detection/process/loopback/availability 测试 |
| Service status | 无 | 0 | 缺少 indicator/decode/retain-error 测试 |
| Settings/UI | 无 | 0 | 没有 View/interaction/accessibility snapshot 或端到端测试 |
| macOS lifecycle/HUD | 无 | 0 | 没有 AppKit window/Space/Dock/login item 测试 |
| Release/packaging | 无 | 0 | workflow/script 仅被源码审计，未运行 |

## 推荐 TDD 行为基线

这些是给新 Tauri/Rust/Vue 应用的可复制 contract，不是把旧 Swift 结构翻译过去的实现要求。每项在进入实现前应先写测试或明确“本版不做”。

### Quota / Provider

1. 正常 Codex 响应映射 primary/secondary 和 `limit_window_seconds`。
2. weekly-only 响应不产生虚假的 five-hour window。
3. 未知 window duration 保留 unknown。
4. Claude legacy 与 dynamic limits 同语义合并。
5. 动态 model scope 能稳定生成 limit id，不因显示名变化重复。
6. percent 被 clamp 到 0–100，负数或超过 100 不污染 UI。
7. 缺少 reset 时只继承同 id/kind 且仍在未来的旧 reset。
8. 429 产生 backoff，backoff 内手动刷新也不发请求。
9. 周期成功间隔内不重复请求，手动刷新只绕过该间隔。
10. 一个 Provider 失败不会丢掉另一个 Provider 的 snapshot。
11. Provider 失败时旧 snapshot 仍能被 UI 读取，并带 stale/error 元数据。
12. 身份变化会清旧 snapshot/cache，不能新身份配旧额度。
13. refresh in-flight 只产生一个请求，重复调用不排队。
14. Statuspage 的失败不清空 quota snapshot，quota 失败也不伪造 status。
15. Antigravity 未安装、已安装未运行、运行但端口不可用必须是不同状态。

### Credentials / recovery

16. Codex OAuth access token 临期才 refresh，PAT 不尝试 JWT refresh。
17. PAT usage 成功后才回填 account/user/email/plan 身份。
18. Codex refresh 成功写回外部 auth 文件时保持原子性。
19. Imported Codex refresh 绝不写主 Codex auth 文件。
20. Claude refresh 读取外部最新 refresh token 后再请求。
21. Claude `invalid_grant` 后可复读并采用其他客户端刚写入的 token。
22. delegated CLI refresh 与 CLI quota fallback 不能在同一状态机中混淆。
23. secret 永不进入前端 command 返回、日志、普通 JSON cache 或错误文案。

### JSONL / rollup

24. 不完整最后一行不消费 offset，下一次追加后能完整解析。
25. Claude 相同 message id 的 streaming 行只产生一个最终 entry。
26. Claude 无 stop_reason 的 assistant 行不进入最终聚合。
27. Claude cache aggregate 与 5m/1h 细分不超过 aggregate，异常输入要 clamp。
28. Claude seen id 去重跨文件生效。
29. Codex active/archive 同 UUID 只取最新有效文件。
30. Codex unchanged cumulative signature 不产生重复 entry。
31. Codex truncation 清理 model/tier/signature，但保留 conversation identity。
32. UsageAggregator 与 ConversationAggregator 的 Token/request/cost 总和一致。
33. rollup 与 scan state 必须共享 generationID 和 pricing fingerprint。
34. 任一 rollup 写入失败会 invalidate watermark，下一次安全重建。
35. pricing fingerprint 变化会触发重新计算，而非沿用旧 cost。
36. models.dev Fast 与 LiteLLM OpenAI Priority 分开解码，Standard 不得回填 Fast。
37. 缺价刷新 30 分钟冷却持久化；手动更新绕过 24 小时。
38. 无关模型远端价格变化不触发全量重算，相关 Fast 价格变化才失效。

### UI / platform

39. loading、empty、stale、offline、error、cached-success 都有明确展示。
40. 日期范围只影响 Overview/list 查询，不改变 Conversation detail 的全生命周期定义。
41. 隐私模式只隐藏展示字段，不会误删原始数据或安全存储。
42. 菜单栏/托盘入口能在无 snapshot 时展示稳定占位，不闪退或消失。
43. 主窗口、紧凑入口、设置和 HUD 的刷新按钮共享一致的去重语义。
44. macOS/Windows 的安全存储、托盘、开机启动、窗口行为通过平台适配层测试。
45. 新版首次启动不会读取旧 cc-bar 的设置、缓存、Keychain namespace 或 HUD frame。

## 测试缺口的结论

当前单元测试对“协议解析 + 增量扫描 + Pricing”有较好的窄覆盖，但不能证明真实 Provider、Keychain、AppKit 生命周期、登录项、HUD、多屏、网络、CI 打包或双平台行为。**代码已确认、待确认**。新版应先把上面的行为基线拆成 Rust domain tests、provider fixture tests、storage corruption tests、Vue component tests 和 macOS/Windows adapter tests，而不是只复制这 25 个 XCTest 名字。

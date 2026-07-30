# ADR-0021：在线价格目录与 Fast 计价同步 cc-bar

- 状态：**已确认**
- 日期：2026-07-30
- 确认依据：产品所有者明确要求 CC Trace 与更新后的 cc-bar 使用同一套价格获取、优先级、缺价刷新和 Fast 计价逻辑，并授权按顺序完成代码、测试与文档。
- 相关文档：[产品范围](../产品范围.md)、[信息架构与核心流程](../信息架构与核心流程.md)、[数据存储与用量索引](../数据存储与用量索引.md)、[技术架构](../技术架构.md)、[测试策略](../测试策略.md)

## 背景

原实现只有随应用内置的一份价格表。它能离线计价，但有四个结构性缺口：

1. 新模型或新 Fast 档位必须等应用发版，无法在模型发布后及时补价。
2. 旧版 `pricing-catalog.json` 会遮住新版应用内置价格，导致应用升级但本地价格不升级。
3. Fast 只有费用，没有独立的计费等效 Token 与倍率事实。
4. Claude 缺失 speed、Codex Cache Write 格式差异和 Claude Cache Write aggregate／明细并存时可能产生错误计价。

cc-bar 已在 2026-07-30 接入 models.dev Fast 与 LiteLLM Priority 字段，并形成经过验证的在线目录策略。CC Trace 的 SQLite 架构与 cc-bar 的 JSON rollup 不同，但价格来源、优先级与安全语义应保持一致。

## 决策

### 1. 价格来源与优先级

- 内置目录始终随二进制加载，不再由磁盘文件替代。
- `pricing-catalog.json` v2 只保存 LiteLLM 与 models.dev 的 ETag、成功／失败时刻、Standard／Codex Fast／Claude Fast 远端费率和缺价刷新冷却。
- Standard：本地历史／阶梯／固定特殊规则 > LiteLLM > models.dev > 本地离线兜底。
- Fast：本地历史／已审计特殊规则 > models.dev Fast > LiteLLM OpenAI Priority > 本地离线兜底。
- 不把 Standard 价格当作 Fast；不从第三方转售商价格推导官方价格。

### 2. 刷新与一致性

- 正常成功后 24 小时刷新一次；某一来源失败后该来源 30 分钟内不再重试。
- 已有用量但缺少 `(source, model, speed)` 价格时立即触发刷新；同一键 30 分钟内最多一次，冷却持久化。它可以绕过 24 小时成功间隔，但不得绕过来源失败退避。
- 设置页提供独立“更新价格目录”，绕过 24 小时和失败冷却；两个来源只成功一个时明确报告“部分更新”，不得写成目录已是最新。
- 网络刷新只写 pending；扫描期间 active 不变。扫描结束或下一次扫描开始前提交 pending，并只在本机实际出现过的价格键发生变化时重算 SQLite 派生列。
- 更新失败、返回空目录或可疑大幅缩小时保留上一份有效价格。Fast 费率使用 last-known 合并，避免上游删除当前型号后破坏历史日志计价。
- 远端条目必须具备 input／output 基础价；缺少 cache read／cache write 子价格按 `$0` 处理。这是产品计价口径，不把它视为未知模型或未知 Fast 价格。

### 3. Fast 等效 Token

- 六维原始 Token 永远不改写。
- Fast 另存计费等效 Token 与倍率。Codex 使用 cc-bar 已审计的 ChatGPT credit 倍率；Claude 使用已知倍率，或仅在 Standard／Fast 四项非零费率比例完全一致时安全推导。
- 无法确认倍率时等效 Token 标为未知，界面显示 `—`，不得从单一输入费率猜测。
- Codex 输入严格超过 272K 时 Fast 继续未定价；Standard 使用模型对应的长上下文阶梯。

### 4. 解析修正

- Claude usage 缺少 speed 时写 `unknown` 并不计价。
- Codex Cache Write 兼容顶层与 `input_tokens_details`、`prompt_tokens_details`、`token_details` 中的字段。
- Claude `cache_creation_input_tokens` 存在时作为写入总量，1h 明细先截断到总量，剩余归 5m；aggregate 缺失时才相加 5m／1h 明细。

## 与 cc-bar 的架构差异

cc-bar 的价格变化会使 rollup 缓存失效并重扫原始日志。CC Trace 已由 [ADR-0018](ADR-0018-用量数据用SQLite与JSON分域.md) 将 Token 事实保存到 SQLite，所以相同行为结果通过数据库内重计价完成，**不会重读 JSONL**。这是存储实现差异，不是计价逻辑差异。

## 代价与风险

- 首次刷新新增两次公开 HTTPS 请求；共享客户端超时仍服从现有参数基线。
- SQLite schema 升至 v2，新增 Fast 派生列与价格指纹状态；v1 必须事务迁移。
- 远端目录错误可能影响费用，因此必须保留官方 Provider 白名单、旧价回退、可疑 shrink 防护和本地特殊规则。
- 本轮代码与测试已补，但未运行自动化、平台构建或双平台实机验证；不得据此写成已验证。

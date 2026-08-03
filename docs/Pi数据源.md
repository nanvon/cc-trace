# Pi 数据源（用量）

> 状态：事实文档；Pi 用量功能属于[产品范围](产品范围.md)的「后续版本评估」，尚未进入首版实现。
>
> 本文件拥有：**Pi coding agent 会话 JSONL 的读取协议、entry 类型与字段语义、增量与去重规则、价格口径、会话元数据与时间规则**。
>
> 事实来源：cc-bar `Core/Usage/PiJSONLScanner.swift` 及相关改动（2026-08-03 提交 `7420786`，当日由产品所有者加入），叠加本机真实会话抽样（2026-08-03，552 条 assistant 消息）。cc-bar 代码只作事实来源，不作需求；实现时以本文件为准，不需要再读 cc-bar 源码。

## 1. 数据源位置与文件格式

- 根目录：`~/.pi/agent/sessions/`；第一级子目录是 cwd 的脱敏目录名（如 `--Users-nanvon-Code-cc-trace--`）。
- 文件：`<ISO时间戳>_<UUID>.jsonl`，append-only JSONL，一行一个 entry。
- 每条 entry 必有 `type`、`id`（短 ID，如 `2a75675c`）、`timestamp`（ISO 8601 UTC 字符串）。
- 递归枚举根目录下全部 `.jsonl`；pi 没有 Codex 那样的 `archived_sessions` 同级目录。

## 2. entry 类型与计入规则

| type | 处理 |
|---|---|
| `session` | 只取 `id` 与 `cwd` 作为会话元数据，不产生用量 |
| `message` 且 `message.role == "assistant"` | **计入**：读 `message.usage`、`message.provider`、`message.model` |
| `message` 且 `message.role == "user"` | 不计入；首条消息文本作为会话标题兜底 |
| `compaction` / `branch_summary` | 仅当**根级**（不在 message 内）带 `usage` 时计入（生成摘要的 LLM 开销）；模型沿用文件内最近一条 assistant 的标签 |
| 其余（`toolResult` 嵌套 usage、`label`、`model_change` 等） | 不参与 |

注意：`toolResult` 里嵌套的 usage **不计**——pi 的 tool 调用输出已包含在所属 assistant 消息的 `usage` 里，单独累加会重复计费。

## 3. `usage` 与 `cost` 字段语义

`message.usage` 结构（2026-08-03 真实抽样）：

```json
{
  "input": 19134,
  "output": 137,
  "cacheRead": 0,
  "cacheWrite": 0,
  "reasoning": 52,
  "totalTokens": 19271,
  "cost": {
    "input": 0.00267876,
    "output": 0.00003836,
    "cacheRead": 0,
    "cacheWrite": 0,
    "total": 0.00271712
  }
}
```

- Token 事实：`input` / `output` / `cacheRead` / `cacheWrite` / `reasoning`（output 子集）/ `totalTokens`。
- **`cost` 嵌在 `usage` 里面**（`usage.cost`）；顶层 `message.cost` 为 `null`，cc-bar 也不读它。
- `cost` 单位是**美元小数**（Decimal），不是整数 nanos；cc-trace 落库时需换算。
- 本机抽样已验证（552 条 assistant 消息）：
  - 全部带 `usage` 与 `cost`（缺 cost 0 条）；
  - `totalTokens == input + output + cacheRead + cacheWrite` 全部成立（0 条不一致）；
  - 551/552 带 `reasoning` 字段。
- 防御规则：`totalTokens <= 0` 且无 `cost` 的行丢弃；解析失败的 entry 跳过不猜值。

## 4. 增量扫描与去重协议

- **watermark**：每文件 `(mtime, byteOffset)`；mtime 与 size 都未变则跳过该文件（会话元数据从 watermark 恢复）。
- **截断防御**：`offset > size`（append-only 下不应发生）时 offset 回 0 重扫，由全局去重兜底防重复计费。
- **全局去重键**：`entryID@entryISO时间戳`，跨文件共享——`/fork`、`/clone` 会把旧行复制进新文件，必须全局去重。
- seen 集合上限 20000 条（超出保留最近 20000，与 Claude scanner 一致）。
- 文件被删除时清理对应 watermark。
- 只消费完整换行行（从 offset 起读完整行，半行留给下次）。

## 5. 价格口径

- **pi 会话自带 cost，不走本地/在线价格表**：cc-bar 对 `app == .pi` 的 `price(for:)`、Fast 倍率、`isMissingPrice()` 全部短路返回 nil／false——缺价**不触发远端刷新**，也不参与价格目录 fingerprint。
- `cost` 是 pi 自己算的：pi 用 `~/.pi/agent/models-store.json` 的模型价格（如 `deepseek-v4-flash`: input 0.14 / output 0.28 / cacheRead 0.0028，美元/百万 Token）在会话时实时计算，写进 JSONL。
- pi 无 Fast 档位概念，speed 恒为 `standard`。
- cc-bar 对缺 cost 的消息：`costUSD = nil`，聚合按 `0` 计入，桶内 `hasUnpricedUsage = true` 仅供诊断。**cc-trace 不得照抄这个行为**（与「未知价格不得按 0」冲突，见 §8）。

## 6. 会话元数据

- 会话键：`pi:<session id>`（session entry 的 `id`；缺失时用文件名末尾 UUID 兜底）。
- 标题兜底：首条 user 消息的纯文本或 `text` 类型 content；空白折叠、去掉 `<` 前缀、截 80 字符；无则空。
- 项目：session `cwd` 经项目解析器解析为**脱敏**项目提示（cc-bar 用 `ConversationProjectResolver`，source 为 cwd）；不得保存明文路径。
- 模型标签：`provider/model`（如 `deepseek/deepseek-v4-flash`）；provider 缺失时只有模型名。
- 识别：cc-bar 的 `UsageApp` 增加 `.pi` case，并有独立识别色 PiAccent。

## 7. 时间与归日

- 消息时间优先 `message.timestamp`（Unix 毫秒）；缺失时退回 entry 的 ISO 时间戳。
- 归日按机器**本地日历日**（与 cc-trace 现有 `day_local` 规则一致）。

## 8. cc-trace 实现时的映射与决策点

| 项 | cc-bar 行为 | cc-trace 规则 | 处置 |
|---|---|---|---|
| 六维 Token 映射 | input/output/cacheRead/cacheWrite 四维 | uncached_input / output / reasoning_output / cache_read / cache_write_5m / cache_write_1h | input→uncached_input；output→output；reasoning→reasoning_output（子集不重复加总量）；cacheWrite 归属 5m 还是 1h **待确认** |
| totalTokens 校验 | 无显式校验（抽样全一致） | Codex 有 `total == input + output` 校验 | 建议复用：不一致则丢弃该行；**待确认**是否含 reasoning（reasoning ⊆ output，不影响和） |
| 模型缺失 | 写字符串 `"unknown/unknown"` | 模型缺失写 `NULL`，禁止伪装 unknown | 需决策：按 cc-trace 规则写 NULL |
| 缺 cost | 按 0 计入 + hasUnpricedUsage 标记 | 未定价费用为 `NULL`，不得按 0 | **需决策**：倾向按 cc-trace 规则（未定价），与 cc-bar 行为不同 |
| 价格目录 | 完全不参与 | 不参与 fingerprint、缺价刷新、重计价 | 保持不参与 |
| 来源标识 | `UsageApp.pi` | `usage_entries` 需扩展 pi 来源列（现仅 codex/claude） | 实现时扩展 |
| 扫描水位 | `ScanState` v10 增加 `pi` / `piSeenEntryIds` | SQLite `scan_files` 水位表 | 实现时扩展水位行 |
| 对话元数据 | `ConversationSeed` key `pi:<id>` | `conversations` 表 | 实现时扩展 |

## 9. 证据等级

| 事实 | 证据 |
|---|---|
| 文件位置、entry 类型、字段结构、去重键、watermark | cc-bar 代码（7420786）+ 真实会话抽样 |
| totalTokens 一致性、全部带 cost、reasoning 出现率 | 本机 552 条真实消息抽样（2026-08-03） |
| deepseek-v4-flash 价格与 `models-store.json` 一致 | 本机 `~/.pi/agent/models-store.json` + cc-bar `Pricing.swift` |
| cacheWrite 窗口语义（5m/1h 归属） | 未验证，待确认 |
| 缺 cost 消息的真实出现率 | 本机抽样 0 条，不代表长期为 0；实现仍需按未定价处理 |

# 本地用量 JSONL Fixture

这些文件是按 2026-07-30 只读抽样确认的字段形态人工构造的脱敏输入，不是真实日志副本。

安全处理：

- session、message、project 均为虚构值；
- 不含 access token、refresh token、Cookie、邮箱、account id 或真实本机路径；
- assistant 消息不含正文，只保留解析 Token 必需的最小字段。

覆盖与预期：

| 文件 | 覆盖 | 预期 |
|---|---|---|
| `codex/session.jsonl` | session identity、model／tier、累计值重复、standard → priority | 三条 `token_count` 只形成两条事实；缓存 Token 从 input 中扣除，priority 标准化为 `fast` |
| `claude/project.jsonl` | 5m／1h cache write、US inference、重复 message id | 两行只形成一条事实；唯一索引去重，两个缓存 TTL 独立 |

半行、截断与同尺寸改写由测试把这些最小行复制到临时目录后构造，仓库 Fixture 本身保持合法 JSONL。

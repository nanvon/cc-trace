# Fixtures

本目录用于保存 CC Trace 自己的脱敏测试输入。

- 只收录首版 Provider 额度解析和错误恢复所需样本。
- 不复制真实 access token、refresh token、Cookie、账号标识或本机路径。
- 从 cc-bar 提取样本时，只引用业务事实和预期结果，不复制旧缓存格式作为新应用输入。
- 每个 Fixture 必须说明来源类型、脱敏方式、覆盖的行为和预期结果。

当前已加入：

- `providers/codex/`：按已确认协议字段和旧版只读测试预期人工构造的 Codex
  Usage API 样本；不是真实账号响应，详细来源与预期见目录内 `README.md`。
- `providers/claude/`：按已确认的 legacy／dynamic 字段语义和旧版只读测试预期人工
  构造的 Claude Code OAuth Usage 样本；不是真实账号响应，详细来源与预期见目录内
  `README.md`。

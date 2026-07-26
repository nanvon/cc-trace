# Claude Code Usage Fixtures

本目录只保存 CC Trace 的脱敏离线测试输入，不是用户数据，也不是真实 Provider
响应的副本。

## 来源与脱敏

- 来源类型：根据 `docs/额度领域模型.md` 第 3.2 节、`docs/cc-bar-reference/`
  的只读协议审计，以及旧版 Claude quota 单元测试中已确认的字段语义人工构造。
- 所有百分比、时间戳、模型名和 scope id 都是测试值，不代表真实账号、真实模型目录
  或 Provider 保证。
- 样本不包含 access token、refresh token、请求头、邮箱、account id、user id、
  Cookie 或本机路径。
- 测试只读取本目录文件，不依赖相邻的 `../cc-bar` 仓库。

## 样本

| 文件 | 覆盖行为 | 预期 |
|---|---|---|
| `usage-mixed.json` | legacy 与动态 session／weekly／scoped 同时存在 | 动态窗口优先，legacy 只补缺失 reset，同语义窗口不重复 |
| `usage-legacy-only.json` | 只有 `five_hour`、`seven_day`、Opus、Sonnet | 四类 legacy 字段映射为标准窗口，5 小时窗口为主要额度 |
| `usage-scoped-and-unknown.json` | model／surface scope、重复 scope id、未知 kind | scope id 稳定派生、重复项更新原位置、未知窗口保留为 `unknown` |

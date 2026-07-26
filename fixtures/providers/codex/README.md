# Codex Usage Fixtures

本目录只保存 CC Trace 的脱敏离线测试输入，不是用户数据，也不是真实 Provider
响应的副本。

## 来源与脱敏

- 来源类型：根据 `docs/额度领域模型.md` 第 3.1 节，以及旧版只读参考中已确认的
  Codex 解析字段和单元测试预期人工构造。
- 所有百分比、时间戳和计划名都是测试值，不代表真实账号或 Provider 保证。
- 样本不包含 access token、refresh token、请求头、邮箱、account id、user id、
  Cookie 或本机路径。
- 测试只读取本目录文件，不依赖相邻的 `../cc-bar` 仓库。

## 样本

| 文件 | 覆盖行为 | 预期 |
|---|---|---|
| `usage-normal.json` | 5 小时主窗口、周次要窗口、绝对与相对 reset | 生成两个稳定窗口，5 小时窗口为主要额度 |
| `usage-weekly-only.json` | 临时只返回周窗口、次要窗口为 `null` | 不虚构 5 小时窗口，周窗口成为主要额度 |
| `usage-unknown-window.json` | 未识别窗口长度、百分比超界、reset 缺失 | 保留 `unknown`、剩余百分比 clamp 为 `0`、reset 保持缺失 |

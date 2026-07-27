# ADR-0014：token 刷新结果回写外部凭据

- 状态：已确认
- 日期：2026-07-27
- 相关文档：[额度领域模型](../额度领域模型.md) 第 5.2 节、[产品范围](../产品范围.md)「数据边界」、[技术架构](../技术架构.md)「凭据与权限」、[ADR-0013](ADR-0013-macOS读取ClaudeCode钥匙串凭据.md)

## 背景

第 4 阶段固化的数据边界包含「不写回、不修改、不删除外部凭据文件」，[额度领域模型](../额度领域模型.md) 第 5.2 节据此规定「首版不回写外部凭据文件，刷新结果只保留在内存中，进程退出即失效」，并把「若 Provider 对频繁刷新有限制需要重新评估」标注为待确认。

进入第 12 阶段时复核 OAuth 刷新的实际语义，发现「刷新但不回写」不是克制，而是有害：

- **Claude Code 的 `refresh_token` 会轮换。** 一次成功刷新后，服务端作废旧的 refresh_token 并下发新的。CC Trace 若不回写，用户的 `~/.claude/.credentials.json` 或 Keychain 里仍是已作废的那份，Claude Code CLI 下次刷新会收到 `invalid_grant` 并掉登录。**CC Trace 的只读监控会直接破坏用户的主力工具登录态。**
- Codex 的刷新响应同样可能返回新的 refresh_token；不回写会产生同类竞态，只是概率较低。
- 不刷新（只用凭据文件里现成的 access token）可以避免上述问题，但 Claude Code 的 access token 寿命较短，日常会频繁进入凭据类 `error`，把「额度一眼可见」变成「大部分时间提示去 CLI 重新登录」。

## 决策

推翻「不写回外部凭据文件」这一条数据边界，**仅针对 token 刷新结果**：

- access token 距过期少于提前量（Codex `300` 秒、Claude Code `30` 秒）时，用 refresh_token 续期。
- 续期成功后，把新的 access token、refresh token 与过期时刻**原子回写到读取它的同一个来源**：Codex 写 `auth.json`，Claude Code 写 `~/.claude/.credentials.json` 或 macOS Keychain 项，来源由 [ADR-0013](ADR-0013-macOS读取ClaudeCode钥匙串凭据.md) 的发现顺序决定。
- 回写只更新 token 三件套与过期时刻，保留文件或 Keychain payload 中的其余字段。
- 发起刷新请求前重新读取凭据来源；若其他客户端已写入更新的 token，直接采用，不发请求。
- 同一 Provider 进程内只允许一个刷新任务，其余调用等待同一结果。
- 刷新失败归为凭据类 `error`，不伪装成 `offline`。

数据边界的其余部分不变：**不创建、不删除外部凭据文件，不写入 token 之外的任何字段，不读取或写入 Swift 版 cc-bar 的任何数据。** 首版仍不提供账号导入，不新增 CC Trace 自有的凭据存储。

首版仍不实现 [ADR-0007](ADR-0007-首版不实现CLI兜底与delegated-refresh.md) 排除的 delegated refresh：刷新被服务端拒绝（`invalid_grant`）时，等待一次并重读来源，若发现其他客户端已写入新 token 就静默采用，否则报凭据类 `error`，不启动外部进程自愈。

## 理由

- 回写是 OAuth refresh token 轮换语义的必然要求，不是额外的写入权限扩张。真正的选择是「刷新并回写」与「完全不刷新」，「刷新不回写」是错误选项。
- Codex CLI 与 Claude Code 自身就是这样处理的；CC Trace 与它们共享同一份凭据，采用相同协议才不会互相踢掉登录。
- 完全不刷新会把常见的 token 过期变成用户可见的错误状态，与「额度一眼可见」的产品定位直接冲突。
- 写入范围极窄：同一来源、同一 payload、只改 token 字段、原子替换。失败时保留原文件。

## 替代方案

| 方案 | 不采用的原因 |
|---|---|
| 完全不刷新，过期即报 error | Claude access token 寿命短，日常大量时间处于错误态 |
| 刷新但不回写 | 会作废用户 CLI 的 refresh_token，破坏主力工具登录态 |
| 只刷新 Codex，不刷新 Claude | 两个 Provider 行为不一致，且 Claude 恰恰是更需要刷新的那个 |
| 把刷新到的 token 存进 CC Trace 自有安全存储 | 与外部来源产生第二份真值，仍无法避免外部 refresh_token 被作废 |

## 后果

- `docs/额度领域模型.md` 第 5.2 节改写，「不回写」结论与对应待确认条目撤销。
- `docs/产品范围.md` 与 `docs/技术架构.md` 的数据边界表述需要标明这一例外及其范围。
- `CLAUDE.md` 第 5.3 节「不写回、不修改、不删除外部凭据文件」需要收窄为「除 token 刷新结果外」。
- 回写路径必须原子替换并保留原有权限位；写入失败时保留原文件，并把本次刷新按凭据类 `error` 处理。
- 持有 token 的类型必须手动屏蔽 `Debug`，回写路径不得进入日志，见 [日志与诊断](../日志与诊断.md)。
- 实机走查必须验证：CC Trace 刷新后，Codex CLI 与 Claude Code CLI 仍能正常使用；两者刷新后 CC Trace 也能继续工作。
- 第 4 阶段「固化数据边界」的结论被本决策局部推翻，执行清单对应条目需要注明例外。

## 复审条件

- 若实机发现 Codex 的刷新响应从不轮换 refresh_token，可以把 Codex 侧收窄为「只在 access token 过期时刷新且不回写」，但必须先有实测证据，不得凭推测收窄。
- 若回写与外部客户端出现无法通过「请求前重读」消解的竞态，重新评估礼让窗口与软恢复策略，而不是自动退回不刷新。
- 若未来引入账号导入，导入账号的 token 必须写进 CC Trace 自有安全存储，不得回写外部来源，届时更新本决策的适用范围。

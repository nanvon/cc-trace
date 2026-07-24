# 04 · 账号、凭据、Provider 与额度协议

## 统一额度模型

`Core/Quota/QuotaModels.swift` 把不同 Provider 标准化为：

```text
QuotaSnapshot
  app
  primaryLimit
  secondaryLimit
  modelLimits[]
  geminiWindow / geminiWeekly    (Antigravity 可选)
  planType
  fetchedAt

QuotaLimit
  id / kind / displayName / window / isActive

QuotaWindow
  usedPercent / resetsAt / windowSeconds
  remainingPercent = clamp(100 - usedPercent, 0...100)
```

`QuotaLimitKind` 有 `fiveHour`、`weekly`、`modelWeekly`、`unknown`。未知窗口必须保留为 `unknown`，不能猜成 5 小时；主窗口/周窗口选择和显示层依靠 `kind`/`id` 而不是字段名硬编码。**代码已确认、测试已确认**。

网络失败的快照策略：错误更新 error/refresh state，保留已有 snapshot；成功新快照如果相同 limit 仍没有 reset，但旧 reset 仍在未来，则沿用旧 reset。**代码已确认**。

## 凭据模型和发现顺序

| Provider | 本地来源 | 账号元数据 | Token 生命周期 | 证据 |
|---|---|---|---|---|
| Codex 主账号 | `CODEX_HOME/auth.json`，否则 `~/.codex/auth.json` | OAuth JWT payload 中的 email、plan、account id/user id；PAT 本地没有这些信息，需 usage 响应回填 | OAuth access/refresh/id token 只在内存中使用，接近过期时回写 auth JSON；PAT 无 refresh | **代码已确认** |
| Claude Code | `~/.claude/.credentials.json`，否则 macOS Keychain 的 Claude Code credentials；email 可由 `~/.claude.json` 兜底 | email、subscription、expiresAt | OAuth refresh token 轮换；刷新成功按原来源原子回写；Keychain 先读再 merge | **代码已确认** |
| Antigravity | 本机安装/App 进程及 Language Server loopback | 本地接口返回 email/plan | 不保存 OAuth token；每次从本机 Language Server 读取 ephemeral CSRF/端口信息 | **代码已确认** |
| Imported Codex | 用户粘贴的 auth JSON/数组/PAT；元数据文件 + Keychain token | 从 JSON JWT 或一次 usage 请求得到 email/plan/account/user id | 不回写主 `~/.codex/auth.json`；OAuth 写导入账号 Keychain，PAT 直接验证 | **代码已确认** |

`CodexAccount` 和 `ClaudeAccount` 的 access/refresh/id token 是内存字段，代码注释明确不写入 UserDefaults。**代码已确认**。文档和输出不得复制真实 Token、邮箱、account id、Keychain JSON 或日志中的敏感字段。

## Codex

### Usage 请求

客户端：`Core/Quota/CodexQuotaClient.swift`。

```text
GET https://chatgpt.com/backend-api/wham/usage
Authorization: Bearer <access token>
Accept: application/json
User-Agent: codex-cli
ChatGPT-Account-Id: <OAuth account id>  (PAT 不带)
```

**代码已确认**：解析 `plan_type`、`rate_limit.primary_window`、可选 `secondary_window`；每个窗口读取 `used_percent`、`reset_at`、`reset_after_seconds`、`limit_window_seconds`。`limit_window_seconds` 推断 5H/weekly，否则 `.unknown`；响应中的 account_id/user_id/email 可用于 PAT 身份回填和身份变化判断。

### OAuth / PAT

- `CodexAuth.load()` 解析 tokens；JWT 只解 payload，不验证签名，用于本地展示 claims。**代码已确认**。
- access token 距过期少于 300 秒时调用 `CodexTokenRefresher` 的 OAuth refresh endpoint；成功原子写回主 auth JSON，导入账号写 Keychain。**代码已确认**。
- PAT 是不透明字符串，没有 JWT/refresh/account id；直接作为 Bearer 请求，usage 响应负责补齐身份。**代码已确认**。
- 主账号身份变化（account id/email 或 PAT access token）会清除旧 quota/cache。**代码已确认**。

### Reset Credits

`CodexResetCreditsClient` 另调 `wham/rate-limit-reset-credits`，解析 available count 和 credit 的 status/title/granted/expires。它是设置页展开时的即时查询：不进入 `QuotaSnapshot`、quota cache、history 或 Scheduler。**代码已确认**。

## Claude Code

### OAuth usage 请求

客户端：`Core/Quota/ClaudeQuotaClient.swift`。

```text
GET https://api.anthropic.com/api/oauth/usage
Authorization: Bearer <access token>
anthropic-beta: oauth-2025-04-20
timeout: 30s
```

**代码已确认**：兼容 legacy `five_hour`、`seven_day`、`seven_day_opus`、`seven_day_sonnet`，也读取新的 generic `limits`：

- `session` → 5 小时主窗口；
- `weekly_all` → 全模型周窗口；
- `weekly_scoped` → `modelLimits`，从 scope.model/surface 生成显示名/id；
- `percent`、reset、is_active 都保留到统一模型。

同一语义窗口会合并 legacy 与动态字段；缺失 reset 可以由 AppState 的未来 reset 保留规则补齐。**代码已确认、测试已确认**。

### Token refresh 和 delegated refresh

- `ClaudeTokenRefresher` 的 skew 是 30 秒，不是文档中泛化的 5 分钟。**代码已确认**。
- Coordinator actor 保证进程内只发一个 refresh；请求前重新读取 credentials，文件最近 10 秒变化时礼让 1 秒；`invalid_grant` 后等待并复读，若别的客户端已写入新 token 则采用。**代码已确认**。
- 仍失败时 `ClaudeDelegatedRefresh` 可在 macOS 上找到 Claude CLI，通过受控 probe 目录、PTY 和可选 watchdog 触发 CLI 自己完成受信任刷新；冷却 5 分钟，成功通知 AppState 再做 quota 刷新。**代码已确认**。
- delegated refresh 不是 quota fallback；它解决 Token 恢复，quota fallback 解决手动查询 API 失败。**代码已确认**。

### CLI quota fallback

`ClaudeCLIFallbackQuotaClient` 只有以下条件成立时调用：用户手动整体刷新、Claude API 已失败、且当前没有 Claude snapshot。它运行 `claude` CLI 的 `/usage`、`/status`、`/exit` 交互，清理 ANSI 后解析 Current session、Current week all models、Sonnet/Opus 等标签和 reset 文案。**代码已确认**。

当前 fallback 在调用前先设置 10 分钟冷却；成功写入 snapshot 时来源是 `.cliFallback`。`storeClaude` 只有 `.api` 成功会清除既有 backoff，因此“fallback 成功后是否应清除相同退避”是可记录的实现风险，不应在新版隐式复制。**代码已确认、待确认**。

## Antigravity

客户端：`Core/Quota/AntigravityQuotaClient.swift`。

1. 检测 `/Applications` 与用户 Applications 下的 Antigravity/Antigravity IDE。
2. 通过 `ps` 只筛 language_server、匹配应用路径/IDE flags 和 `--csrf_token` 的进程。
3. 通过 `lsof` 找 loopback 端口。
4. 对 `127.0.0.1` 的 HTTPS 端口调用 `RetrieveUserQuotaSummary` / `GetUserStatus`，带 `X-Codeium-Csrf-Token` 和 `Connect-Protocol-Version: 1`。
5. 仅对 127.0.0.1 接受本地自签名证书；URLSession timeout 约 6 秒。

返回结果按 Claude+GPT 与 Gemini 分组，得到主 5H/weekly、Gemini 5H/weekly、email/plan 和配置模型 fallback。状态是 notInstalled / installed / running / unavailable(detail)。**代码已确认**。

新版若要支持 Windows，不能直接移植路径和 `ps`/`lsof`；必须重新定义进程发现、loopback 协议、证书策略和平台安全边界。**合理推断**。

## Public service status

`ServiceStatusClient` 访问 OpenAI 与 Anthropic 的公开 Statuspage `status.json`，解析 `none`、`minor`、`major`、`critical`、`maintenance`、`unknown`，结果只有内存字段，没有磁盘 cache。**代码已确认**。Provider quota 的失败不等于 Statuspage 的故障，两条状态链不能合并。

## 错误分类

`QuotaError` 包括 missingToken、HTTP、transport、decode、tokenRefreshFailed、tokenRevoked；并提供 isRateLimited、isAuthFailure 等判断。**代码已确认**。

| 错误 | 当前处理 |
|---|---|
| 无凭据/无 token | Provider state 失败；不发请求；UI 保留已有数据或显示 `--`。 |
| 401/Token revoked | Claude 可进入 delegated refresh；Codex 记录错误；不会把旧额度误标成新身份。 |
| 429 | 记录错误并设置 10 分钟 backoff；手动刷新不绕过。 |
| transport/decode | 保留 snapshot，更新短错误文案；不清空可展示数据。 |
| Antigravity 未安装/未运行 | 先更新 availability，再将 quota 记为 unavailable/error。 |
| statuspage 拉取失败 | 保留上次 status 内存值；不影响 quota snapshot。 |

## Provider 重写契约

新版无论是否保留当前接口，都应先把以下行为写成平台无关的 contract：

- Provider 返回可识别的 limit kind/id/window，不猜窗口；
- `remainingPercent` 统一 clamp；
- snapshot 与 refresh state 分离；
- 网络失败不清空最后可展示 snapshot；
- 身份变化先清旧 snapshot；
- 429 退避不能被手动刷新绕开；
- token refresh 不把 secret 暴露给 Vue；
- delegated CLI recovery 和 quota CLI fallback 分开；
- Provider 失败隔离，不能阻断其他 Provider；
- 所有真实验证仍需独立授权和脱敏 Fixture，当前静态审计不代替真机验证。


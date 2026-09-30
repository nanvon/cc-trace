# ADR-0035：服务矩阵与 Windows 凭据存储

- 状态：已确认
- 日期：2026-10-01
- 相关文档：[ADR-0014](ADR-0014-token刷新结果回写外部凭据.md)、[ADR-0025](ADR-0025-非隐私模式显示完整邮箱.md)、[ADR-0031](ADR-0031-功能基准改为cc-bar-v1.1.1.md)、[ADR-0032](ADR-0032-额度主体与多账号.md)、[额度领域模型](../额度领域模型.md)、[日志与诊断](../日志与诊断.md)、[桌面壳验证记录](../桌面壳验证记录.md)

## 背景

[ADR-0031](ADR-0031-功能基准改为cc-bar-v1.1.1.md) 把功能基准改为 cc-bar v1.1.1，额度服务从两个扩到五个，设置页也从「一列开关」变成按服务分组的四组开关矩阵。随之而来两个平台问题：

1. **凭据从哪来**：五个服务的凭据来源与形态各不相同，其中三个在 Windows 上落在与 macOS 不同的位置或不同的系统接口上。
2. **我们自己的秘密放哪**：导入的 Codex 副账号凭据与手动填写的 Command Code API Key 没有外部真源，必须由我们自己保存；macOS 有钥匙串，Windows 上没有同样的东西。

## 决策

### 1. 设置的服务矩阵

`services` 按服务分组，每服务四组独立开关：

| 开关 | 作用 |
|---|---|
| `quota` | 额度卡片与后台额度轮询 |
| `menuBar` | 系统区域（macOS 菜单栏徽标／Windows 托盘 tooltip）是否承载该服务 |
| `hud` | 桌面悬浮窗是否显示该服务行 |
| `stats` | 是否计入本地用量统计（侧栏筛选、KPI、图表、对话与项目页） |

默认值与 cc-bar 一致：Codex 与 Claude Code 全开；Antigravity 检测到登录凭据时开启；Cursor 与 Command Code 默认关闭。Pi／OpenCode／DSH 没有额度，共用 `localAgentStats` 一个统计开关。

关闭 `quota` 的服务不发请求，界面继续展示上一次快照（不假装数据消失）。

### 2. 五个服务的凭据来源

| 服务 | 来源（按优先级） | 写回 |
|---|---|---|
| Codex 主账号 | `~/.codex/auth.json`（OAuth 或 PAT） | 续期后按 [ADR-0014](ADR-0014-token刷新结果回写外部凭据.md) 原子回写同一文件 |
| Codex 副账号 | 用户粘贴的 `auth.json` 或 PAT，存 CC Trace 自己的秘密存储 | 续期后回写同一槽位 |
| Claude Code | `~/.claude/.credentials.json` → macOS 钥匙串 `Claude Code-credentials`；账号档案（邮箱／accountUuid／organizationUuid）读 `~/.claude.json` | 续期后回写读到的那一个来源 |
| Claude Desktop | Electron `safeStorage` 缓存，**只借 access token** | 不回写；永不读取或使用它的 refresh token |
| Antigravity | `~/.gemini/jetski-standalone-oauth-token` → `~/.gemini/oauth_creds.json` | 续期后按 ADR-0014 原子回写同一文件 |
| Cursor | 只读 Cursor 的 `state.vscdb`（`cursorAuth/accessToken`） | 不回写、不复制 |
| Command Code | `~/.commandcode/auth.json` → `~/.pi/agent/auth.json` → OpenCode `auth.json` → 环境变量 → 手动 API Key | 不回写 |

「Claude Desktop 只借 access token」是一条硬规则：Anthropic 的 refresh token 一次性轮换并带重用检测，任何一方拿它刷新都会把用户从另一端挤下线；access token 是只读凭证，借来调 usage 端点不产生轮换。

Antigravity 的 **client secret 不硬编码**：续期时从本机已安装的官方组件（Gemini CLI 的 npm bundle、`~/.gemini/bin/agy`）里提取 `GOCSPX-` 密钥，按「紧邻在前」优先与 client id 配对。读的是用户自己机器上的官方组件，因此不受 Google 轮换影响，也不必把官方密钥带进我们的仓库与发行包。

### 3. 我们自己的秘密：系统存储，不落明文文件

| 平台 | 实现 | 说明 |
|---|---|---|
| macOS | 系统钥匙串（`security-framework`，service `cc-trace`） | 条目名 `cc-trace/<槽位>` |
| Windows | 凭据管理器（`CredReadW` / `CredWriteW` / `CredDeleteW`） | 目标名 `cc-trace/<槽位>`，`CRED_TYPE_GENERIC` + `CRED_PERSIST_LOCAL_MACHINE` |

Windows 侧的结构体与常量取自 `windows-sys` 官方绑定（由 Windows 元数据生成），**不手抄 `CREDENTIALW` 的字段顺序或取值**。blob 按平台约定编码为 UTF-16LE，这样在系统凭据界面里看到的是可读文本而不是乱码。

槽位名由身份短哈希派生（`codex-imported-<16 位十六进制>`），**不用下标**：删除或重排账号时下标会变，用下标做槽位会把 A 的凭据读成 B 的。

没有系统秘密存储的平台（非 macOS／非 Windows）明确返回「被拒」，**不退回明文文件**。

### 4. Windows 路径与接口的取值（证据等级）

| 项 | 取值 | 证据等级 |
|---|---|---|
| Codex `auth.json` | `%USERPROFILE%\.codex\auth.json` | 高：CLI 自家目录相对路径，全平台一致 |
| Claude Code 凭据 | `%USERPROFILE%\.claude\.credentials.json` | 中：Windows 上没有钥匙串，社区与上游实现一致；未实机验证 |
| Claude Desktop 配置 | `%APPDATA%\Claude\config.json`；MSIX 安装时另试 `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude\config.json` | 中：Electron 应用惯例 + 社区实现；未实机验证 |
| Claude Desktop 解密 | DPAPI（`CryptUnprotectData`） | 高：Electron 官方文档写明 Windows 的加密密钥由 DPAPI 生成、与当前登录凭据绑定 |
| Cursor `state.vscdb` | `%APPDATA%\Cursor\User\globalStorage\state.vscdb` | 中：VS Code 分叉的既定布局与第三方实现；未实机验证 |
| Antigravity `~/.gemini` | `%USERPROFILE%\.gemini\...` | 中：与 Gemini CLI 同源的家目录约定；未实机验证 |
| Command Code `auth.json` | `%USERPROFILE%\.commandcode\auth.json` | 中：cc-bar 用家目录拼接，Windows 上未验证 |

**Windows 侧全部行为在实机验证前标注未验证**，按 [桌面壳验证记录](../桌面壳验证记录.md) 的规则记录平台与日期。

## 理由

- 服务矩阵把「要不要看」与「要不要扫」分开：用户关掉某个服务的额度卡片，不该连带丢掉它的用量历史；关掉统计也不该停掉额度轮询。一个开关管两件事，就必然要为其中一件做错。
- 用系统凭据存储而不是自建加密文件：密钥本身也要有地方放，落在应用数据目录里等于把锁和钥匙放在同一个抽屉；而两个平台的系统存储都是「当前用户可见、不需要管理员、可在系统界面里自行删除」的。
- 槽位用哈希不用下标：导入账号是可以删除与重排的，用位置做标识意味着一次重排就把凭据张冠李戴。
- 不硬编码 Antigravity 的 client secret：官方密钥会轮换，而把密钥带进仓库与发行包没有任何必要——本机官方组件里就有一份，且永远与官方同步。

## 替代方案

1. **统一用 `keyring` 之类跨平台库**。被否决：仓库已经在 macOS 上直接用 `security-framework`，再加一层抽象会同时丢掉两边的错误码细节（例如钥匙串的授权被拒与凭据管理器的不存在是两种不同的处置）。
2. **自建加密文件存我们的秘密**。被否决：密钥无处安放；且用户无法在系统界面里审计与撤销。
3. **Antigravity 不做 client secret 提取，直接带一个内置密钥**。被否决：等于把官方密钥随应用分发，且密钥轮换即全体失效。
4. **只支持 macOS 侧的凭据来源，Windows 上让用户手动填 token**。被否决：这不是等价实现，是空壳。

## 后果

- `platform::secret_store` 是「我们自己的秘密」的唯一入口；`providers::credentials` 继续只读外部来源（Codex／Antigravity 的续期回写是唯一例外，见 ADR-0014）。
- 设置 schema 升到 v2；v1 的 `usageServiceVisibility` 读入即迁移，不再写回。
- 五个服务各有自己的契约测试与脱敏夹具；新增服务时按同一张表补齐「来源、写回、Windows 位置」三列。

## 复审条件

- Windows 实机验证发现某个路径或接口与上表不同（尤其是 MSIX 虚拟化路径与 DPAPI 的可解性）。
- Electron／Chromium 在 Windows 上改用 master key + AES-GCM（那会让 Claude Desktop 这条路径的解密方式变化）。
- cc-bar 再次扩张服务集合。

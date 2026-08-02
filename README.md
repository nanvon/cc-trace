<p align="center">
  <img src="design/brand/cc-trace-lockup-horizontal.svg" alt="CC Trace" height="64">
</p>

<p align="center">
  在菜单栏／托盘里看一眼，就知道 Codex 和 Claude Code 的额度还剩多少。
</p>

<p align="center">
  <img src="https://img.shields.io/github/v/release/nanvon/cc-trace?style=flat-square&color=0b7285" alt="Release">
  <img src="https://img.shields.io/badge/platform-macOS%2013%2B%20%7C%20Windows%2010%2B-555?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/github/actions/workflow/status/nanvon/cc-trace/ci.yml?style=flat-square&label=CI" alt="CI">
  <img src="https://img.shields.io/badge/license-MIT-555?style=flat-square" alt="License">
</p>

<p align="center">
  <a href="https://github.com/nanvon/cc-trace/releases/latest"><b>↓ 下载最新版本</b></a>
  &nbsp;·&nbsp;
  <a href="https://github.com/nanvon/cc-trace-mobile">移动端</a>
  &nbsp;·&nbsp;
  <a href="docs/README.md">开发文档</a>
</p>

---

CC Trace 常驻 macOS 菜单栏或 Windows 系统托盘，显示 Codex 与 Claude Code 的额度余量、重置时间和数据新鲜度。不用打开网页后台，也不用敲诊断命令。

打开主窗口，还能基于 Codex 与 Claude Code 自己写下的本地 JSONL，看清 Token 用量、对话活动，以及按公开 API 目录估算的等值费用。

**适合谁**：在个人电脑上使用 Codex、Claude Code 或两者的开发者；想快速判断额度是否紧张、又不想频繁打开 Provider 网页的人。前提是你已经在本机登录过其中至少一个 —— CC Trace 自动发现本机凭据，**不提供登录页，也不能用来注册或切换账号**。

> [!WARNING]
> 仍在开发中，功能与体验可能继续调整。**Windows 侧尚未实机验证。**

<!-- 截图占位：请补充以下两到三张图后删除本注释。
     建议放在 docs/images/ 下，宽度 1200px 左右，深浅色各一套更好。
     1. tray-compact.png   菜单栏／托盘紧凑入口（两个 Provider 并排 + 余量分档着色）
     2. usage-window.png   主窗口本地用量页（每日花费图 + 按模型明细表）
     3. tray-stale.png     可选：拿不到新数据时的旧快照／离线状态，体现「数据诚实」
-->

|                  紧凑入口                   |                   本地用量页                   |
| :-----------------------------------------: | :--------------------------------------------: |
| _待补充 `docs/images/tray-compact.png`_ | _待补充 `docs/images/usage-window.png`_ |

## 功能

- **两个 Provider 并排** — 自动识别本机已登录的 Codex 与 Claude Code 账号，不需要在应用里再登录一次
- **余量分档着色** — 菜单栏／托盘图标旁直接显示关键数字，紧张时不点开也能发现
- **重置倒计时** — 每项额度什么时候恢复，以及上一次成功刷新的时间
- **本地用量统计** — 只读增量扫描 `~/.codex/sessions`、`~/.codex/archived_sessions` 与 `~/.claude/projects` 下的 JSONL，按 Token、模型、速度和对话聚合
- **等值费用估算** — 按公开 API 价格目录估算相同 Token 的 API 标价；未知模型或缺价明确显示「未定价」，不按 `0` 蒙混
- **主窗口是用量页** — 日期快捷选择与自定义范围、总 Token 与总花费、两个 Provider 各自的用量、按 Provider 堆叠的每日花费图，以及按模型的明细表
- **自动加手动刷新** — 额度按 15／30／60 分钟间隔刷新（默认 30 分钟），随时可以手动点一下；用量索引在后台每 5 分钟自动增量扫描
- **数据诚实** — 拿不到新数据时明确区分旧快照、离线、限流和出错，不把过期数据伪装成最新，也不会在读不到账号时显示一个假的 `0%`
- **故障隔离** — 一个 Provider 请求失败不会拖累另一个
- **中英文、深浅色、开机自启** — 都可以跟随系统或在设置里切换

## 安装

从 [Releases](https://github.com/nanvon/cc-trace/releases/latest) 下载：

| 平台 | 文件 | 要求 |
|---|---|---|
| macOS Apple Silicon | `CC-Trace_<版本>_macOS-Apple-Silicon.dmg` | macOS 13 或更高 |
| macOS Intel | `CC-Trace_<版本>_macOS-Intel.dmg` | macOS 13 或更高 |
| Windows x64 | `CC-Trace_<版本>_Windows-x64-Setup.exe` | Windows 10 22H2 或更高（64 位） |

不确定自己的 Mac 是哪种：点击左上角  → 「关于本机」，看「芯片」一行。两个 macOS 平台除 DMG 外还各提供一个同名 `.zip`，内容相同，免挂载。

每个版本附 `SHA256SUMS.txt`，可校验完整性：

```bash
# macOS
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
```

需要你已经在本机登录过 Codex 或 Claude Code。只装了一个也能用，另一个会显示为「未发现凭据」。

Windows 版依赖系统的 WebView2 运行时。Windows 11 与较新的 Windows 10 已内置；若缺失，安装程序会**联网下载**并安装它，所以首次安装请保持网络畅通。

CC Trace 不购买 Apple 开发者账号和 Windows 代码签名证书，**首次打开需要绕过系统提示**：macOS 在「访达」里右键图标选「打开」；Windows 在 SmartScreen 提示里选「更多信息 → 仍要运行」。这是当前发布边界的预期表现，不代表出了问题。

<details>
<summary>自行构建</summary>

<br>

需要 Node.js 22+、pnpm 11、Rust 稳定版工具链，以及 [Tauri 官方要求的平台依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
pnpm install
pnpm build:mac:release
```

产物在 `src-tauri/target/release/bundle/macos/`。

</details>

## 快速开始

1. **首次启动**会走一段简短引导，做最小状态检查并说明需要的权限。macOS 上首次读取 Claude Code 的钥匙串凭据时，系统会弹出授权窗口，请选择「**始终允许**」。
2. 引导完成后，CC Trace 常驻 **macOS 菜单栏**或 **Windows 系统托盘**。点一下图标打开紧凑入口，两个 Provider 的额度余量、重置倒计时和数据新鲜度一屏读完。
3. 图标旁会直接显示关键数字并按余量分档着色，**不点开也能发现额度紧张**。
4. 引导完成后会立即执行首次用量扫描，之后每 5 分钟自动增量扫描。扫描进度可在界面上看到，首次索引在后台执行，不阻塞额度显示。
5. 从紧凑入口进入**主窗口**查看本地用量：Token 用量、对话活动和按公开 API 目录估算的等值费用。

只登录了一个 Provider 也能正常使用，另一个显示为「未发现凭据」，不会显示成 `0%`。

## 隐私

这类工具要碰你的登录凭据，所以边界写在前面：

- **完全在本机运行** — 没有自己的服务器，只直接访问 Codex 与 Claude Code 官方接口，凭据不会被发送到任何第三方
- **只读外部数据源** — 不提供登录页，也不导入浏览器 Cookie、不抓取网页；只读取 Codex／Claude Code 自己保存的凭据和 JSONL，用量索引只写入 CC Trace 自己的数据目录
- **对外部数据的唯一写入是续期** — 凭据即将过期时按官方 OAuth 流程续期，并把结果写回原来的位置，除此之外不修改、不删除你的任何凭据或日志
- **凭据不进界面、不进日志** — 处理凭据的代码全在 Rust 内核里，界面只拿到脱敏后的账号信息和额度数字；access token、refresh token 一律不写入日志文件和缓存
- **开源可查** — 以上每一条都可以在本仓库源码里核对

macOS 上首次读取 Claude Code 的钥匙串凭据时，系统会弹出授权窗口，请选择「**始终允许**」；选「允许」只对这一次生效，下次还会再问。

<details>
<summary>常见问题</summary>

<br>

**它显示的费用是我实付的钱吗？**
不是。那是把相同 Token 按公开 API 标价换算的**估算值**，不是 Codex／Claude 订阅实付、账单或发票。口径见[数据存储与用量索引](docs/数据存储与用量索引.md)。

**能管理多个账号吗？**
不能。首版只跟踪本机当前登录的一个 Codex 身份和一个 Claude Code 身份，不支持账号切换或导入。

**为什么额度显示成「旧数据」？**
说明最近一次刷新没成功，界面会同时说明原因（离线、限流、出错等）和这份数据的时间。手动刷新一次通常就能恢复。

**和之前的 cc-bar 是什么关系？**
cc-bar 是同一作者更早的 Swift 版 macOS 菜单栏应用。CC Trace 是全新的跨平台重写版，两者是完全独立的应用（标识、数据目录、缓存都不共用）。它不会迁移 cc-bar 的设置和数据，也不会自动卸载它，两者可以同时装在一台机器上。

</details>

## 已知限制

**额度接口随时可能失效。** CC Trace 直接访问 Codex 与 Claude Code 自身使用的用量接口，它们不是对外承诺的公开 API。Provider 调整返回结构或收紧策略时，额度查询会失效或解析不出结果。这是这条实现路径的固有代价，技术上无法消除 —— 发生时界面会明确说明失败原因，而不是拿旧数据充数。

**只跟踪本机当前登录的身份。** 一个 Codex 身份加一个 Claude Code 身份，不支持多账号、账号切换或导入。

**费用是估算，不是账单。** 按公开 API 目录换算相同 Token 的标价，与订阅实付、发票无关。

**Windows 侧尚未实机验证。** 代码路径已实现且 CI 在 Windows 上跑通编译与测试，但托盘、窗口和多显示器行为需要真实桌面会话才能验收。

**不做的事**：团队配额与组织账单管理、云端账号中心、通用 AI 聊天客户端、Provider 网站的 WebView 包装。

## 文档

产品与工程文档从 [文档地图](docs/README.md) 开始，那里说明每份文档的职责与事实归属。

技术栈：Tauri 2 + Vue 3 + TypeScript + Rust。

## 免责声明

本项目不是 OpenAI 或 Anthropic 的官方产品，与两家公司无关，也未获得其认可或支持。Codex、Claude 及相关名称归各自所有者。

## 许可证

[MIT](LICENSE)

<p align="center">
  <img src="design/brand/app-icons/app-icon-master-1024.png" width="128" alt="CC Trace Logo">
</p>

<h1 align="center">CC Trace</h1>

<p align="center">
  <b>跨平台桌面端 AI 额度监控与本地用量看板</b><br>
  macOS 菜单栏 / Windows 托盘常驻，实时掌握 Codex 与 Claude Code 配额水位，透视多引擎本地 Token 消耗与费用走势。
</p>

<p align="center">
  <img alt="Platform" src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-000000?logo=apple&logoColor=white">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-v2-24C8DB?logo=tauri&logoColor=white">
  <a href="https://github.com/nanvon/cc-trace/releases/latest"><img alt="Latest Release" src="https://img.shields.io/github/v/release/nanvon/cc-trace?color=brightgreen"></a>
  <img alt="Downloads" src="https://img.shields.io/github/downloads/nanvon/cc-trace/total?color=blue">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-orange">
</p>

<p align="center">
  <a href="https://github.com/nanvon/cc-trace/releases/latest">下载安装</a> ·
  <a href="#-核心特性">核心特性</a> ·
  <a href="#-界面预览">界面预览</a> ·
  <a href="#-快速安装">安装指南</a> ·
  <a href="#-数据与隐私安全">数据安全</a> ·
  <a href="#-从源码构建">从源码构建</a> ·
  <a href="#-相关项目">相关项目</a> ·
  <a href="https://github.com/nanvon/cc-trace/issues">问题反馈</a> ·
  <a href="README_EN.md">English</a>
</p>

> [!WARNING]
> 本项目处于快速迭代期（`0.1.x` 版本），核心额度抓取与会话扫描能力已完备，功能与交互细节可能根据真实使用反馈微调。Windows 侧持续推进实机体验适配。

---

## ✨ 核心特性

### ⚡ 跨平台常驻与紧凑额度
- **双平台原生入口** — 支持 macOS 菜单栏与 Windows 系统托盘常驻，macOS 菜单栏直显服务剩余百分比，Windows 托盘 Tooltip 秒级呈现额度水位于状态指示灯。
- **毛玻璃浮动面板** — 点击即出紧凑卡片窗口，原生失焦自动收起（macOS Transient Popover 特性），集中展示 Codex 与 Claude Code 的 5 小时主额度、7 天周额度与重置倒计时。
- **服务健康监控** — 实时读取 OpenAI 与 Anthropic 官方公开 Statuspage 状态链，在提供商卡片头部分流呈现服务运行状态圆点。
- **故障隔离与智能退避** — 独立管理各 Provider 网络请求周期，内置 429 速率限制退避机制与双服务故障隔离，遇到断网或接口异常时自动保留上一份有效快照。

### 📊 本地会话用量与费用透视
- **多引擎会话解析** — 自动增量扫描 Codex、Claude Code、Pi 及 OpenCode 四类 AI 编码助手的本地会话文件（只读解析 JSONL 与 SQLite 日志）。
- **多维指标钻取** — 支持按今日、昨日、近 7 天、近 30 天、本周、本月、本年或自定义时间跨度筛选，提供输入、输出、推理输出与缓存命中率拆解，配备每日堆叠柱状图。
- **全生命周期会话追踪** — 独立 Conversations 视图支持按标题、项目检索与多字段排序，展示单次会话起止时间、耗时、Git 分支、Token 分布与 API 等值换算费用。
- **额度历史时间线** — 独立 Timeline 视图记录每次刷新时额度整数值的阶梯变化，利用不可逆指纹隔离账号序列，直观回溯配额消耗速率。

### 🛡️ 纯净可靠的桌面架构
- **零配置开箱即用** — 自动探测本机已有终端凭据，无需重复输入 API Key 或登录账号；单服务凭据缺失时正常呈现另一服务，不产生阻断性报错。
- **轻量低耗与系统融合** — 基于 Tauri v2 与 Rust 2024 构建，搭配 Vue 3 贴合式界面，原生内存占用低；支持开机自启、深浅色模式与系统语言自适应。

---

### 📸 界面预览

| 菜单栏 / 托盘额度面板 | 本地用量统计与会话分析 |
| :---: | :---: |
| _待补充 `docs/images/popover-light.png`_ | _待补充 `docs/images/statistics-overview.png`_ |

> [!NOTE]
> 界面截图与动图素材制作中。正式分发包已包含完整的深色/浅色自适应毛玻璃紧凑面板、多维用量图表与会话详情视图。

---

## 📦 快速安装

🍎 **系统环境要求**：
- **macOS**：macOS 13.0 (Ventura) 或更高版本，原生支持 Apple Silicon (M 系列) 与 Intel 架构；
- **Windows**：Windows 10 22H2 或更高版本（64 位），依赖 Microsoft Edge WebView2 运行时（Windows 11 已内置）。

### 1. 获取预编译安装包
从 [Releases 页面](https://github.com/nanvon/cc-trace/releases/latest) 下载适配本机的安装产物：

| 操作系统 / 架构 | 发布文件 | 说明 |
| :--- | :--- | :--- |
| **macOS Apple Silicon** | `CC-Trace_<version>_macOS-Apple-Silicon.dmg`<br>`CC-Trace_<version>_macOS-Apple-Silicon.zip` | 适用于 M1/M2/M3/M4 系列芯片 Mac（免挂载推荐 `.zip`） |
| **macOS Intel** | `CC-Trace_<version>_macOS-Intel.dmg`<br>`CC-Trace_<version>_macOS-Intel.zip` | 适用于 x86_64 架构 Intel 芯片 Mac |
| **Windows x64** | `CC-Trace_<version>_Windows-x64-Setup.exe` | 64 位 NSIS 安装引导程序（缺失 WebView2 时自动联网下载） |

### 2. 系统放行与权限授权
由于开源构建产物未向 Apple 购买商业开发者证书公证、未向微软购买代码签名证书，首次运行被系统拦截属于正常保护机制：

* **macOS Gatekeeper 放行**：打开应用提示拦截后，进入 **系统设置 → 隐私与安全性**，下滑找到 CC Trace 的拦截提示，点击 **「仍要打开」**。
* **macOS 钥匙串授权**：首次读取 Claude Code 凭据若弹出钥匙串授权框，请选择 **「始终允许」**（若选单次「允许」，每次重启都会重复弹窗）。
* **Windows SmartScreen 放行**：安装时若弹出 SmartScreen 蓝色防护卡片，点击 **「更多信息」→「仍要运行」**。

> [!NOTE]
> 自 macOS Sequoia 起，旧版「右键 → 打开」快捷绕过方式已被系统弃用，必须通过上述系统设置手动放行。
> 若系统持续报错提示「应用已损坏」，可打开终端执行以下命令清除隔离属性：
> ```bash
> xattr -dr com.apple.quarantine "/Applications/CC Trace.app"
> ```

每个版本随附 `SHA256SUMS.txt` 校验清单，可通过终端命令核验安装包完整性：
```bash
# macOS
shasum -a 256 -c SHA256SUMS.txt --ignore-missing

# Windows (PowerShell)
Get-FileHash CC-Trace_*_Windows-x64-Setup.exe -Algorithm SHA256
```

---

## 🔒 数据与隐私安全

CC Trace 为纯本地运行的个人开源监控工具。为了统计额度与分析会话，它严格在本地最小权限范围内访问系统文件：

### 凭据与权限透明度
| 服务 / 模块 | 凭据与数据存储位置 | 访问权限 | 行为机制与安全保障 |
| :--- | :--- | :---: | :--- |
| **Codex** | `~/.codex/auth.json` | 读 / 写 | 仅读取本机已生成的 OAuth 凭据，临期时原子回写更新 `refresh_token` 与刷新元数据，绝不修改其他字段 |
| **Claude Code** | `~/.claude/.credentials.json`<br>macOS Keychain (系统钥匙串) | **读 / 写** | 优先读取凭据文件，macOS 缺失时回退系统钥匙串；凭据临期原子更新，不向任何第三方转发凭据明文 |
| **会话日志扫描** | `~/.codex/sessions`<br>`~/.claude/projects`<br>`~/.pi/agent/sessions`<br>`~/.local/share/opencode/` | **严格只读** | 仅按行增量扫描 Token 与时间戳，绝不修改、截断、移动或删除原始会话文件 |
| **本地用量索引** | 应用私有数据目录 `usage.db` (SQLite) | 读 / 写 | 聚合指标与额度历史仅保存在本机私有数据库，采用不可逆指纹隔离身份，严禁明文凭据与消息正文入库 |

### 核心隐私保证
- **零网络遥测（Zero Telemetry）** — 不包含任何分析打点、行为追踪或第三方监控 SDK；外部网络请求严格仅发往 OpenAI 与 Anthropic 官方额度接口及公共 Statuspage 状态页。
- **内核隔离与内存安全** — 敏感凭据仅在 Rust 内核处理并封装于 `Secret` 结构体中，屏蔽标准输出与调试日志，不向 Vue 前端暴露 token 明文；前端仅展示脱敏后账号与数值。
- **纯净文本解析** — 不抓取网络原始报文，不解密网络流量，会话正文与项目绝对路径永远不跨越 IPC 边界进入前端。
- **透明费用估算** — API 等值费用仅根据公共模型标价换算，供本地参考，不代表实际订阅账单。

> [!TIP]
> 预编译安装包直接由 GitHub Actions 官方开源工作流构建。若您对未签名二进制程序有安全顾虑，推荐自行审查代码并[从源码构建](#-从源码构建)。

---

## 🔧 从源码构建

### 环境准备
- **Node.js**：`22.x` 或更高版本
- **pnpm**：`11.x` 或更高版本
- **Rust**：Stable 工具链（支持 2024 Edition）
- **平台基础依赖**：遵循 [Tauri 2 Prerequisites](https://v2.tauri.app/start/prerequisites/)（macOS 需安装 Xcode Command Line Tools，Windows 需安装 Visual Studio C++ 生成工具）。

### 本地开发与调试
```bash
# 克隆仓库
git clone https://github.com/nanvon/cc-trace.git
cd cc-trace

# 安装前端依赖
pnpm install

# 启动本地开发环境（同时拉起前端 Vite 与 Tauri 桌面壳）
pnpm tauri dev
```

### 生产打包分发
```bash
# macOS: 编译并生成 .app、.dmg 与免安装 .zip
pnpm build:mac:release

# Windows: 编译并生成 64 位 NSIS 安装程序 (.exe)
pnpm tauri build
```
编译产物位于 `src-tauri/target/release/bundle/` 对应的平台子目录中。

---

## 🔗 相关项目

同一作者构建的 AI 额度监控套件，共享相同的额度核算口径与设计语言：

| 项目 | 形态 / 技术栈 | 适用平台与场景 |
| :--- | :--- | :--- |
| [**cc-bar**](https://github.com/nanvon/cc-bar) | macOS 原生菜单栏应用 (SwiftUI) | 极致轻量、原汁原味的 macOS 状态栏看板与桌面 HUD |
| **CC Trace** (本仓库) | 跨平台桌面端应用 (Tauri 2 + Rust + Vue 3) | 面向 macOS 菜单栏与 Windows 系统托盘，兼顾多引擎日志透视与深度图表分析 |
| [**cc-trace-mobile**](https://github.com/nanvon/cc-trace-mobile) | 移动端配套应用 (Flutter) | 随时随地在 iOS / Android 移动设备上查阅配额状态与消耗趋势 |

---

## 🙏 致谢

在架构设计、交互体验与数据解析思路上，本项目参考或借鉴了以下优秀开源项目：

- [cc-switch](https://github.com/farion1231/cc-switch) — 多 Provider 账号切换器，启发了凭据发现与多账号管理逻辑。
- [cockpit-tools](https://github.com/jlcodes99/cockpit-tools) — 多平台 AI 编码助手仪表盘，在额度刷新策略与状态呈现上提供了重要参考。
- [CodexBar](https://github.com/steipete/CodexBar) — macOS 菜单栏 AI 用量监控工具，在菜单栏集成与本地日志解析方面提供了先驱实践。
- [Tauri](https://github.com/tauri-apps/tauri) — 轻量高效的跨平台应用构建引擎，为桌面端提供了坚实的底层支撑。

---

## 📢 免责声明

本项目为独立的开源第三方工具，非 OpenAI 或 Anthropic 的官方产品，亦未获得两家公司的官方认可或关联支持。Codex、Claude 及相关商标归其各自所有者持有。

---

## 📄 许可证

本项目基于 [MIT License](LICENSE) 开源。

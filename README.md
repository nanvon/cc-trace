<p align="center">
  <img src="design/brand/app-icons/app-icon-master-1024.png" width="128" alt="CC Trace 图标">
</p>

<h1 align="center">CC Trace</h1>

<p align="center">macOS 菜单栏 / Windows 托盘工具:实时显示 Codex 与 Claude Code 的剩余额度,<br>并统计本机的 Token 用量与费用。</p>

<p align="center">
  <img alt="macOS" src="https://img.shields.io/badge/macOS-13%2B-000000?logo=apple&logoColor=white">
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%2B-0078D4">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-24C8DB?logo=tauri&logoColor=white">
  <a href="https://github.com/nanvon/cc-trace/releases/latest"><img alt="release" src="https://img.shields.io/github/v/release/nanvon/cc-trace?color=brightgreen"></a>
  <img alt="license" src="https://img.shields.io/badge/license-MIT-orange">
</p>

<p align="center">
  <a href="https://github.com/nanvon/cc-trace/releases/latest">下载</a> ·
  <a href="#-安装">安装</a> ·
  <a href="#-从源码构建">从源码构建</a> ·
  <a href="#-相关项目">相关项目</a> ·
  <a href="https://github.com/nanvon/cc-trace/issues">反馈</a> ·
  <a href="README_EN.md">English</a>
</p>

<!-- 头部截图占位:补充 docs/images/ 下的深浅色额度面板截图后替换本注释:
<p align="center">
  <img src="docs/images/popover-light.png" width="360" alt="额度总览 - 浅色模式">
  <img src="docs/images/popover-dark.png" width="360" alt="额度总览 - 深色模式">
</p>
-->

> [!WARNING]
> 仍在开发中,功能与体验可能继续调整。**Windows 侧尚未实机验证。**

## ✨ 功能

- **额度总览** —— Codex 与 Claude Code 的剩余额度、重置倒计时与今日 / 本周费用,点击菜单栏 / 托盘图标即可查看;自动识别本机已登录的账号,无需在应用内重复登录
- **菜单栏 / 托盘显示** —— macOS 在菜单栏图标旁直接显示两个服务的剩余百分比,Windows 显示在托盘 tooltip;右键菜单提供打开、刷新、设置与退出
- **用量统计** —— 汇总 Codex 与 Claude Code 的 Token 用量与费用,按今天 / 昨天 / 本周 / 本月 / 本年 / 近 7 天 / 近 30 天 / 全部 / 自定义范围切换,支持按服务、按模型拆分,并提供按服务堆叠的每日花费图;数据来自只读扫描本机会话日志(JSONL),费用按公开 API 价格换算
- **设置项** —— 刷新间隔、中英双语、深浅色、开机自启、价格目录更新
- **首次启动引导** —— 说明用途与凭据边界,检查本机 Codex / Claude Code 凭据;没有凭据也能继续

### 📸 界面预览

<!-- 截图占位:建议补充以下素材(放 docs/images/ 下,深浅色各一套)后替换本表格。
     1. 双主题并排:popover-light.png / popover-dark.png 各 <img width="360">
        (菜单栏 / 托盘额度面板:两个服务并排 + 余量分档着色;同素材可复用到头部截图位)
     2. 大图:statistics-overview.png <img width="720"> + <sub> 一句话图注
        (主窗口用量统计:每日花费图 + 按模型明细表)
-->

|                 额度总览                  |                 用量统计                       |
| :---------------------------------------: | :--------------------------------------------: |
| _待补充 `docs/images/popover-light.png`_ | _待补充 `docs/images/statistics-overview.png`_ |

## 📦 安装

🍎 要求 macOS 13 或更高,或 Windows 10 22H2 或更高(64 位)。Codex / Claude Code 需已在本机完成登录(至少一个)——只装了一个也能用,另一个会显示为「未发现凭据」。

1. 从 [Releases](https://github.com/nanvon/cc-trace/releases/latest) 下载对应平台的文件:

   | 平台                | 文件                                      |
   | ------------------- | ----------------------------------------- |
   | macOS Apple Silicon | `CC-Trace_<版本>_macOS-Apple-Silicon.dmg` |
   | macOS Intel         | `CC-Trace_<版本>_macOS-Intel.dmg`         |
   | Windows x64         | `CC-Trace_<版本>_Windows-x64-Setup.exe`   |

   不确定自己的 Mac 是哪种:点屏幕左上角的苹果菜单 → 「关于本机」,看「芯片」一行。两个 macOS 平台除 DMG 外还各提供一个同名 `.zip`,内容相同,免挂载。

2. macOS:打开 DMG,把 `CC Trace.app` 拖入「应用程序」。Windows:运行安装程序;它依赖系统的 WebView2 运行时,Windows 11 与较新的 Windows 10 已内置,缺失时安装程序会**联网下载**。

3. CC Trace 未做 Apple 公证与 Windows 代码签名,首次打开被系统拦一次是预期表现:macOS 双击被拦下后,到 **系统设置 → 隐私与安全性**,下滑找到 CC Trace 的提示,点 **「仍要打开」**;Windows 在 SmartScreen 提示里选 **「更多信息 → 仍要运行」**。

4. macOS 首次读取 Claude Code 的 Keychain 凭据时会弹出授权窗口,请选 **「始终允许」**——选「允许」只对这一次生效,下次还会再问。

> [!NOTE]
> macOS Sequoia 起,旧的「右键 → 打开」放行方式已失效,只能通过上面的系统设置放行。
> 若仍提示「应用程序已损坏」,可在终端手动去除隔离属性:
>
> ```bash
> xattr -dr com.apple.quarantine "/Applications/CC Trace.app"
> ```

每个版本附 `SHA256SUMS.txt`,可校验下载完整性:

```bash
# macOS
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
```

## 🔒 数据与安全

CC Trace 是为个人需求开发的开源小工具。为了查询额度,它会读取本地凭据:

- Codex:`~/.codex/auth.json`
- Claude Code:`~/.claude/.credentials.json` 与 macOS Keychain
- 只向 Codex 与 Claude Code 的官方接口请求额度数据,凭据不发给任何第三方;对外部数据的唯一写入是按官方 OAuth 流程续期即将过期的凭据
- 凭据只在 Rust 内核里处理,界面只拿到额度数字与账号信息(完整账号仅用于本地展示,见 `docs/决策/ADR-0025`);token 不进日志、不进缓存

用量统计基于只读扫描 Codex 与 Claude Code 的本机会话日志(JSONL)得出,索引只写入 CC Trace 自己的数据目录。

> [!TIP]
> 发布的产物为 ad-hoc 签名、未做 Apple 公证,Windows 侧未做代码签名;如果介意,可以自行审阅代码后[从源码构建](#-从源码构建),不依赖发布的二进制包。

## 🔧 从源码构建

技术栈:Tauri 2 + Vue 3 + TypeScript + Rust。需要 Node.js 22+、pnpm 11、Rust 稳定版工具链,以及 [Tauri 官方要求的平台依赖](https://v2.tauri.app/start/prerequisites/)。

**日常开发**:`pnpm install` 后 `pnpm tauri dev`。

**打包分发**:

```bash
pnpm build:mac:release   # macOS:产出 .app、.dmg 与 .zip
pnpm tauri build         # Windows:产出 NSIS 安装包
```

产物在 `src-tauri/target/release/bundle/` 对应子目录。产品与工程文档从[文档地图](docs/README.md)开始。

## 🔗 相关项目

同一作者的三个应用,共享同一套额度口径与视觉语言:

|                                                                  |                                        |
| ---------------------------------------------------------------- | -------------------------------------- |
| [**cc-bar**](https://github.com/nanvon/cc-bar)                   | macOS 原生菜单栏版(SwiftUI)          |
| **CC Trace**(本仓库)                                           | 桌面端 · macOS 菜单栏 / Windows 托盘   |
| [**CC Trace Mobile**](https://github.com/nanvon/cc-trace-mobile) | 移动端 · iOS / Android                 |

CC Trace 在 cc-bar 的功能基础上用 Tauri 重构,同时支持 macOS 与 Windows。三个应用相互独立,数据与设置不互通。

## 🙏 致谢

设计与实现参考了以下开源项目(经由前身 [cc-bar](https://github.com/nanvon/cc-bar) 延续至本项目):

- [cc-switch](https://github.com/farion1231/cc-switch) —— 多 Provider 账号切换器,启发了多账号管理与导入流程
- [cockpit-tools](https://github.com/jlcodes99/cockpit-tools) —— 多平台 AI 编码助手仪表盘,在额度与刷新策略上提供了参考
- [CodexBar](https://github.com/steipete/CodexBar) —— macOS 菜单栏 AI 用量监控,在菜单栏交互与本地解析思路上多有借鉴

## 📢 免责声明

本项目不是 OpenAI 或 Anthropic 的官方产品,与两家公司无关,也未获得其认可或支持。Codex、Claude 及相关名称归各自所有者。

## 📄 许可证

[MIT](LICENSE)

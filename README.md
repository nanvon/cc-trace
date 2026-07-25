# CC Trace

CC Trace 是一款面向使用 Codex 与 Claude Code 的开发者的跨平台桌面应用，让用户通过 macOS 菜单栏或 Windows 系统托盘，快速查看 AI 编程工具的额度、重置时间、刷新状态与异常风险。

当前处于 `0.1.0` 工程基线阶段：产品范围和技术边界已经固化，Tauri 2、Vue 3、TypeScript、Rust、Pinia、Vue Router 与 Vue I18n 的最小框架已经建立；Provider 与正式业务页面尚未开始实现。

## 首版边界

首版聚焦：

- Codex 与 Claude Code 当前额度。
- macOS Menu Bar 与 Windows System Tray。
- 手动／自动刷新、节流、429 退避和故障隔离。
- `loading`、`live`、`stale`、`offline`、`error` 状态。
- 独立设置、最新有效快照和基础偏好。

首版不包含本地用量、Conversations、Timeline、Pricing、其他账号和桌面悬浮窗，也不读取或迁移 Swift 版 cc-bar 的应用数据。

## 文档入口

- [产品定义](docs/产品定义.md)
- [首版产品范围](docs/产品范围.md)
- [技术架构](docs/技术架构.md)
- [执行清单](docs/Tauri桌面端重新开发执行清单.md)
- [AI 协作与 Skills 规范](AGENTS.md)
- [cc-bar 只读参考资料](docs/cc-bar-reference/README.md)
- [品牌与跨端图标](design/brand/README.md)

## 技术栈

- Tauri 2
- Vue 3 + TypeScript + Vite
- Pinia + Vue Router + Vue I18n
- Rust
- pnpm + Cargo

## 本地环境

- Node.js 22 或更高版本
- pnpm 11
- 当前稳定 Rust 工具链
- Tauri 对应平台依赖

完整平台准备说明见 [Tauri 官方 prerequisites](https://v2.tauri.app/start/prerequisites/)。

## 常用命令

```bash
pnpm install
pnpm tauri dev
pnpm build
```

仓库初始化阶段没有自动运行开发服务器、构建或测试。执行这些命令前，请先确认当前工作目标确实需要。

## 目录

```text
src/                    Vue 应用、功能模块、状态、路由、国际化与样式
src-tauri/              Rust 核心、Tauri 配置、capabilities 与桌面图标
docs/                   产品、范围、架构、执行清单与旧版参考
design/brand/           CC Trace 品牌母版与跨端图标源文件
```

应用标识为 `com.nanvon.cctrace`，与 Swift 版 cc-bar 的 `com.nanvon.ccbar` 完全独立。

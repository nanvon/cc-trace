# CC Trace

CC Trace 是一款面向使用 Codex 与 Claude Code 的开发者的跨平台桌面应用，让用户通过 macOS 菜单栏或 Windows 系统托盘，快速查看 AI 编程工具的额度、重置时间、刷新状态与异常风险。

当前处于 `0.1.0` 工程基线阶段：产品范围、信息架构、设计方向和技术边界已经固化，Tauri 2、Vue 3、TypeScript、Rust、Pinia、Vue Router 与 Vue I18n 的最小框架已经建立。Tray 桌面壳使用合成数据在 macOS 完成实机验证，Windows 实机验证与首次启动窗口尚未完成；Provider、缓存、调度和正式业务页面尚未开始实现。

## 首版边界

首版聚焦：

- Codex 与 Claude Code 当前额度。
- macOS Menu Bar 与 Windows System Tray。
- 手动／自动刷新、节流、429 退避和故障隔离。
- 完整的加载、实时、旧快照、无凭据、不支持、离线、刷新受限与错误状态。
- 独立设置、最新有效快照和基础偏好。

首版不包含本地用量、Conversations、Timeline、Pricing、其他账号和桌面悬浮窗，也不读取或迁移 Swift 版 cc-bar 的应用数据。

## 文档入口

从 **[文档地图](docs/README.md)** 开始：它说明每份文档的职责、每个事实由谁拥有，以及修改文档的规则。

**规范**

- [产品定义](docs/产品定义.md)
- [首版产品范围](docs/产品范围.md)
- [信息架构与核心流程](docs/信息架构与核心流程.md)
- [状态与错误模型](docs/状态与错误模型.md)
- [额度领域模型](docs/额度领域模型.md)
- [设计方向与状态规范](docs/设计方向与状态规范.md)
- [技术架构](docs/技术架构.md)
- [文案与国际化](docs/文案与国际化.md)
- [日志与诊断](docs/日志与诊断.md)
- [测试策略](docs/测试策略.md)
- [工程与发布](docs/工程与发布.md)

**决策**

- [决策记录索引](docs/决策/README.md)

**进度与验证**

- [执行清单](docs/Tauri桌面端重新开发执行清单.md)
- [双平台交互原型](docs/双平台交互原型.md) · [可交互原型页面](prototypes/tray-shell/index.html)
- [Tauri Tray 桌面壳验证记录](docs/桌面壳验证记录.md)

**AI 协作与品牌**

- [AI 协作与 Skills 规范](AGENTS.md)
- [AI 设计产品上下文](PRODUCT.md) · [设计系统种子](DESIGN.md)
- [品牌与跨端图标](design/brand/README.md)

**外部参考**

- [cc-bar 只读参考资料](docs/cc-bar-reference/README.md)
- [历史规划归档](docs/archive/)

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
docs/                   规范、决策记录、进度与验证、外部参考
docs/决策/              ADR：已确认决策的背景、理由与复审条件
docs/archive/           历史规划文档，冲突时以现行规范为准
design/brand/           CC Trace 品牌母版与跨端图标源文件
fixtures/               CC Trace 自己的脱敏测试输入
prototypes/tray-shell/  第 8 阶段双平台交互原型（不属于正式应用）
```

应用标识为 `com.nanvon.cctrace`，与 Swift 版 cc-bar 的 `com.nanvon.ccbar` 完全独立。

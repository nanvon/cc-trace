# ADR-0001：采用 Tauri 2 + Vue 3 + Rust 重写

- 状态：已确认
- 日期：2026-07-25（补写）
- 相关文档：[技术架构](../技术架构.md)、[产品定义](../产品定义.md)

> 本记录事后补写。决策本身在此之前已确认，理由依据现有文档与 [cc-bar 只读参考](../cc-bar-reference/README.md) 重建；若与当时的实际考量不符，请直接更正。

## 背景

Swift 版 cc-bar 是 macOS 专属应用，使用 SwiftUI 与 AppKit，业务核心（Provider 请求、凭据发现、调度、缓存）与 macOS 平台能力耦合。产品需要同时服务 macOS 与 Windows 用户，并保持同一套业务语义。

## 决策

在独立新仓库中使用 Tauri 2、Vue 3、TypeScript、Rust 从零实现，业务核心留在 Rust，界面使用共享 Vue 页面，平台差异集中在 Rust 平台适配层。

## 理由

- 一套 Rust 业务核心可以同时服务两个平台，避免维护两份 Provider 与调度实现。
- 秘密处理需要一个前端拿不到的边界；Rust + 窄 command 边界天然提供这个边界。
- 使用系统 WebView（macOS WKWebView、Windows WebView2），分发体积与常驻内存明显小于捆绑 Chromium 的方案，对一个常驻系统区域的小工具尤其重要。
- Tauri 2 官方提供 Vue + TypeScript 模板与跨平台 Tray、窗口、开机启动能力。

## 替代方案

| 方案 | 不采用的原因 |
|---|---|
| 继续 Swift，另写一份 Windows 实现 | 两套业务核心，规则会长期漂移 |
| Electron | 常驻内存与分发体积不适合系统区域小工具；秘密仍在 JS 进程内 |
| .NET / Avalonia / Flutter Desktop | 团队既有前端能力无法复用，Provider 生态与 WebView 一致性不占优 |
| 在 Swift 版上原地改造 | 平台耦合已深，改造成本高于重写，且不符合“新产品重新定义范围”的前提 |

## 后果

- macOS 与 Windows 的 Tray、窗口生命周期和材质差异必须逐项验证，不能假设一致（见 [桌面壳验证记录](../桌面壳验证记录.md)）。
- WebView 差异（字体、滚动、透明窗口、阴影、焦点）成为新的风险面。
- Swift 版 cc-bar 继续独立存在，只作只读事实来源。

## 复审条件

- Windows WebView2 在紧凑面板上的性能或对比度无法满足要求。
- Tauri 2 的 Tray 或窗口能力在某个平台长期缺失关键行为。

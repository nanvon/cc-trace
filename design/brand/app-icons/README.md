# CC Trace App Icon System

桌面 App 图标已按 [ADR-0030](../../../docs/决策/ADR-0030-App图标改用仓鼠用量轨迹插画.md) 改为仓鼠查看彩色用量轨迹的插画。Logo 与 App 图标不再强制共用同一图形；Tray／Menu Bar 仍使用双 `C` 微型符号。

## 当前母版与生成规则

- `app-icon-master-1024.png`：当前唯一的桌面 App 图标生成母版，1024 × 1024 RGBA PNG。
- 原稿为 1254 × 1254 RGB PNG；处理只清除与四角连通的近黑外底并缩放为 1024 × 1024，不重绘仓鼠、轨迹面板、颜色或构图。
- 使用 Tauri CLI 从该母版生成 `src-tauri/icons/` 下的 PNG、ICNS、ICO 与 Windows Square／Store 资产。
- `src-tauri/icons/tray-symbol.png` 不参与这次生成，避免应用图标替换破坏系统区域的高对比识别。

## 平台职责

- macOS Dock 与 Bundle：`src-tauri/icons/icon.icns`。
- Windows 应用、Start、任务栏与安装包：`src-tauri/icons/icon.ico` 及 `Square*Logo.png`／`StoreLogo.png`。
- Tauri 通用窗口图标：`32x32.png`、`64x64.png`、`128x128.png`、`128x128@2x.png`、`icon.png`。
- macOS Menu Bar／Windows Tray：继续使用 `tray-symbol.png` 与既有平台逻辑，不复用复杂插画。

## 历史资产

- `apple-app-icon-master.svg`、`apple-app-icon-master-1024.png`、`android/` 与 `windows/` 下的双 `C` 资产保留用于设计回溯，不再作为当前桌面 Bundle 图标的生成输入。
- `tray-symbol.svg` 仍是当前系统区域微型符号源。

## 官方规范

- Apple: https://developer.apple.com/design/human-interface-guidelines/app-icons
- Android: https://developer.android.com/develop/ui/compose/system/icon_design_adaptive
- Windows: https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-construction

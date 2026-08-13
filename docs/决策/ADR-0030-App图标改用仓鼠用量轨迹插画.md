# ADR-0030：App 图标改用仓鼠用量轨迹插画

- 状态：已确认
- 日期：2026-08-13
- 相关文档：[设计方向与状态规范](../设计方向与状态规范.md)、[品牌说明](../../design/brand/README.md)、[App 图标系统](../../design/brand/app-icons/README.md)

## 背景

产品所有者提供新的方形 PNG 图标稿，并明确要求 CC Trace 改用该图标。图稿以仓鼠、深色用量面板和彩色轨迹表达「查看多来源用量变化」，与应用的核心任务直接相关。

原图为 1254 × 1254 RGB PNG，圆角插画外侧四角是近黑色实底；若直接生成平台图标，这部分会进入 Bundle 资产。现有 Tauri 图标集同时服务 macOS Dock、Windows 应用／Start／任务栏与安装包，而 macOS Menu Bar／Windows Tray 另有独立的小尺寸高对比资产。

## 决策

1. 桌面 App 图标改用仓鼠查看彩色用量轨迹的插画，不再要求 App 图标与双 `C` Logo 共用同一图形。
2. 只清除与原图四角连通的近黑外底，保留插画内部黑色用量面板、仓鼠、彩色轨迹、颜色与构图；缩放为 1024 × 1024 RGBA 母版 `design/brand/app-icons/app-icon-master-1024.png`。
3. 用 Tauri CLI 从该母版生成 `src-tauri/icons/` 的 PNG、ICNS、ICO 与 Windows Square／Store 资产。
4. `src-tauri/icons/tray-symbol.png` 与动态菜单栏徽标逻辑保持不变。复杂插画不缩进 16–18pt 系统区域；双 `C` 继续承担 Logo、字标与 Tray／Menu Bar 微型识别。
5. 历史双 `C` App 图标源保留用于回溯，不再作为当前桌面 Bundle 图标的生成输入。

## 理由

- 用户指定的新插画是当前产品识别，不应继续让旧双 `C` Bundle 图标覆盖它。
- 插画中的仓鼠与用量轨迹比抽象符号更直接地表达应用用途，并在 Dock／Start 等中尺寸场景提供更强识别性。
- App 图标与系统区域微型图标的阅读距离和像素预算不同。分开处理能保留新插画，同时不牺牲 Menu Bar／Tray 的清晰度。
- 确定性去底、缩放与 Tauri 官方生成链不会重绘或改变用户选定的图稿。

## 后果

- macOS 与 Windows 的应用包、Dock／Start／任务栏及通用窗口图标改用新插画。
- Menu Bar／Tray 的双 `C` 微型图标及额度展示行为不变。
- Logo、字标与页面内少量双 `C` 几何呼应继续有效；App 图标成为独立的插画资产。
- 静态资产检查不能替代重新打包后的 macOS／Windows 实机显示确认。

## 复审条件

- 若 16／32px 的 Windows 应用图标在真实任务栏上无法辨识，可单独增加小尺寸光学校正版，不回退 Dock／Start 的插画方向，也不改 Tray 资产职责。
- 若后续移动端采用该插画，需要按各平台 Adaptive Icon／App Icon 规范单独制作安全区与图层，不直接复用桌面导出结果。

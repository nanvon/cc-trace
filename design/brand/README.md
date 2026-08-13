# CC Trace Logo

这是 CC Trace 已确认采用的 Logo 几何与品牌源文件。桌面 App 图标已按 [ADR-0030](../../docs/决策/ADR-0030-App图标改用仓鼠用量轨迹插画.md) 改用独立插画，不再要求与 Logo 共用同一几何：

- 两个外层 `C` 使用完整圆头，代表 Codex 与 Claude。
- 内部不是小 `C`，而是一长一短两段独立圆弧。
- 内外圆弧严格共用同一个圆心，径向间距在任何位置都保持一致。
- 左向凝视只由内部断弧的分布、长短和旋转角度产生，不依赖圆心偏移。
- 左右内部弧段保留约 `4°–6°` 的旋转差，避免机械复制。
- Logo、字标和 Tray／Menu Bar 微型图标统一使用无尾巴的双 `C`，不附加线段或节点。
- 裸 Logo 使用紧凑的 `200 × 128` 视觉画布，不再为已废弃的 trace 尾巴预留右侧空间。

## 文件

- `cc-trace-symbol.svg`：深色基础符号，透明背景。
- `cc-trace-symbol-reverse.svg`：暖白反白符号，透明背景。
- `cc-trace-lockup-horizontal.svg`：符号与 `CC TRACE` 横向组合。
- `cc-trace-app-icon.svg`、`cc-trace-app-icon-1024.png`：历史双 `C` App 图标源，保留用于回溯，不再生成当前 Bundle 图标。
- `app-icons/`：Apple、Android、Windows 与 Tray/Menu Bar 的跨平台 App 图标系统。

## App 图标系统

当前桌面 App 图标以 `app-icons/app-icon-master-1024.png` 为唯一生成母版。圆角矩形属于这张插画的构图容器；透明外角用于避免把源图黑底带进平台资产：

- Apple Dock／Bundle 与 Windows 应用、Start、任务栏：从插画母版生成对应位图、ICNS 与 ICO。
- Tray / Menu Bar：继续使用独立加粗的双 `C` 微型符号，不把复杂插画缩进 16–18pt 系统区域。
- 旧 Apple／Android／Windows 双 `C` 源文件保留为历史设计资产，不是当前桌面 Bundle 图标的生成输入。

详细说明见 `app-icons/README.md`。

## 几何规则

- 外层圆弧半径：`44`
- 外层线宽：`11`
- 外层开口：约 `90°`
- 内层圆弧半径：`22`
- 内层线宽：`7.5`
- 内外圆弧圆心偏移：`0`
- 内外笔画边缘径向间距：约 `12.75`
- 主弧长度：约 `145°–148°`
- 短弧长度：约 `21°–23°`
- 主弧与短弧之间留白：约 `26°–31°`

所有端点均为圆头。Logo、字标组合和系统区域小尺寸版本使用同一套无尾巴双 `C` 几何；App 图标按 ADR-0030 独立。

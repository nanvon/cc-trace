# CC Trace App Icon System

核心规则：圆角矩形是 App 图标容器，不是 Logo 本体。所有平台继续复用同一组同心双 C 路径。

## Apple

- `apple-app-icon-master.svg`：1024 × 1024 全出血方形母版，不预切圆角；由系统应用最终遮罩。
- `apple-app-icon-master-1024.png`：Apple 平台使用的 1024 × 1024 PNG 导出。
- 双 `C` 约占画布宽度 `62%`，在几何居中基础上轻微向右、向下修正视觉重心。
- 背景使用极轻的中性炭黑明暗变化，不使用彩色渐变、玻璃高光或霓虹。

## Android

- `android/ic_launcher_background.svg`：108 × 108 背景层。
- `android/ic_launcher_foreground.svg`：108 × 108 前景层，Logo 位于中央 66 × 66 安全区域。
- `android/ic_launcher_monochrome.svg`：Android 主题图标单色层。
- 双 `C` 可见宽度约 `60dp`，完整位于中央 `66 × 66dp` 安全区，并加入不影响安全区的轻微光学校正。
- 前景、背景不包含圆角遮罩，由 Launcher 决定圆形、Squircle 或其他外形。

## Windows

- `windows/windows-plated.svg`：Start / Store 等场景的带底板版本。
- `windows/windows-unplated-light.svg`：浅色表面使用的深色透明底版本。
- `windows/windows-unplated-dark.svg`：深色表面使用的暖白透明底版本。
- 带底板版本的双 `C` 约占画布宽度 `63%`；无底板版本约占 `70%`。
- 正式实现时应从矢量母版导出 16、24、32、48、256px 等目标尺寸，并逐级做光学校正。

## Tray / Menu Bar

- `tray-symbol.svg`：32 × 32 加粗微型符号，不带圆角矩形，约占画布宽度 `75%`。
- Tray / Menu Bar 版本按双 `C` 视觉重心校正，并针对 16–32px 使用更粗笔画。

## 官方规范

- Apple: https://developer.apple.com/design/human-interface-guidelines/app-icons
- Android: https://developer.android.com/develop/ui/compose/system/icon_design_adaptive
- Windows: https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-construction

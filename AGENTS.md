# CC Trace AI 协作规范

本文件约束参与 CC Trace 规划、设计、实现和审查的 AI。产品与技术决策仍以以下文档为准：

- `docs/产品定义.md`
- `docs/产品范围.md`
- `docs/技术架构.md`
- `docs/Tauri桌面端重新开发执行清单.md`

AI 开始工作前必须先读取与任务有关的上述文档，不得用 Skill 的通用建议覆盖已经确认的产品范围、技术边界、阶段门禁或用户决定。

## Skills 使用原则

- 任务命中已安装 Skill 的适用范围时，必须先读取该 Skill 的 `SKILL.md`，再开始设计、实现或审查。
- 每次只使用完成任务所需的最小 Skill 集合，不为形式一次性加载所有 Skills。
- 使用 Skill 前应说明名称和用途；Skill 缺失或不可读取时，应明确说明并采用最接近的项目内规范继续。
- Skill 只能改善执行质量，不能自行扩大功能范围、提前跨阶段实现或引入无关重构。
- 未经用户明确要求，不运行 build、lint、type-check、test、dev、浏览器自动化或耗时的平台构建。

## 第 6 阶段：信息架构

执行“重做信息架构”时：

- 使用 `impeccable` 检查信息层级、窗口职责、核心流程、认知负担和异常恢复路径。
- 当职责、边界、验收标准或平台差异仍不明确时，先使用 `grilling` 逐项澄清，不得猜测。
- 明确紧凑额度入口、主窗口、设置、首次启动以及 macOS Menu Bar／Windows Tray 的职责和关系。
- 不在本阶段实现正式业务页面、Tray 桌面壳或 Provider。

## 第 7 阶段：设计方向

执行“确定设计方向”时：

- 使用 `frontend-design` 建立独立、克制、符合 CC Trace 品牌的视觉方向，避免模板化界面。
- 使用 `apple-design` 审查 macOS Menu Bar、窗口、反馈、动效和平台习惯；其结论不能直接套用到 Windows。
- 使用 `impeccable` 覆盖视觉层级、状态表达、响应式、主题、中英文、长文本、键盘和无障碍。
- 状态和微交互进入可实现细节时，使用 `make-interfaces-feel-better` 检查排版、间距、边框、阴影、反馈和 reduced motion。
- 同时给出 Windows Tray 的等价交互，不把 macOS 行为当作跨平台默认值。

## 第 8 阶段：交互原型

执行“完成交互原型”时：

- 使用 `impeccable` 审查紧凑入口、主窗口、设置和首次启动的完整流程。
- 使用 `web-design-guidelines` 审查可访问性、键盘路径、焦点、表单、状态文案和响应式表现。
- 原型视觉完成后使用 `kill-ai-slop` 做一次针对性检查，去除模板化、过度装饰、无意义渐变、徽标堆叠和空泛文案；不得因此改动已确认的信息架构。
- 需要真实页面验证时，取得用户同意后使用 `browser` 或 `playwright` 检查交互、窗口尺寸和状态切换。
- 原型必须覆盖 `loading`、`live`、`no_credentials`、`offline`、`stale` 和 `error`，并覆盖刷新中、刷新成功及保留旧快照。
- macOS Menu Bar 和 Windows Tray 必须使用同一业务语义分别绘制等价线框。

## 正式实现与审查

- 第 6～8 阶段未经用户确认，不开始正式业务页面。
- 交互原型未经用户确认，不实现 Tray 桌面壳。
- macOS／Windows 桌面壳未经验证，不开始 Provider 最小闭环。
- 正式实现 UI 时，按任务继续使用 `frontend-design`、`impeccable`、`apple-design` 或 `make-interfaces-feel-better` 中最相关的 Skill。
- 提交前的 UI 审查使用 `web-design-guidelines`；出现明显模板化或 AI 默认风格时再使用 `kill-ai-slop`，不把它作为每次代码修改的固定步骤。
- Skill 导致的设计选择、风险或与项目文档的冲突必须在交付说明中明确记录。

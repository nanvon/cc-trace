# Product

<!-- impeccable:product-schema 1 -->

本文件为 AI 设计工作提供稳定的产品事实摘要。正式范围和技术边界仍以 `docs/产品定义.md`、`docs/产品范围.md`、`docs/技术架构.md` 与 `docs/Tauri桌面端重新开发执行清单.md` 为准。

## Platform

adaptive

CC Trace 使用同一套业务语义和共享 Vue 页面服务 macOS 与 Windows，同时分别遵循两端的系统区域、窗口生命周期和交互习惯。macOS Menu Bar 与 Windows System Tray 不追求像素级或操作级完全相同。

## Users

- 在个人电脑上使用 Codex、Claude Code，或同时使用两者的开发者。
- 希望快速判断额度是否紧张，而不想频繁打开 Provider 网页或执行诊断命令的用户。
- 需要在 macOS 与 Windows 上获得一致核心能力，同时保留平台得体交互的用户。

首版以单机、单用户和本机凭据自动发现为前提，不面向团队配额管理、云端账号中心或企业审计。

## Product Purpose

CC Trace 让用户从 macOS 菜单栏或 Windows 系统托盘快速判断 Codex、Claude Code 的额度风险，并理解剩余额度、重置时间、刷新状态和异常情况。

成功意味着用户无需进入多层页面，就能完成三件事：

1. 立即发现哪个 Provider 接近额度限制。
2. 分辨数据是实时、加载中、旧快照、离线还是错误。
3. 可靠地手动或自动刷新，并在单个 Provider 失败时保留另一方的正常结果和已有有效快照。

## Positioning

CC Trace 是本机优先、权限克制的跨平台额度观察工具，不是 Provider 登录器、网页包装、聊天客户端或团队管理后台。

它通过 Rust 层发现 Codex、Claude Code 的本机凭据并访问 Provider 接口；token 临近过期时按 OAuth
语义刷新并把结果原子回写同一来源，Vue 只接收脱敏账号信息、额度快照、用量聚合、扫描状态和
展示状态。它拥有独立于 Swift 版 cc-bar 的应用身份、数据目录、缓存和发布体系，不读取、迁移、
覆盖或删除 cc-bar 的应用数据。

## Operating Context

- 应用常驻 macOS Menu Bar 或 Windows System Tray。
- 用户主点击系统区域图标时先打开紧凑额度面板，不直接打开主窗口。
- 用户主要从紧凑入口快速查看额度风险、状态、刷新结果，以及 Codex／Claude Code Token 用量
  的今日／本周 API 等值费用。
- 用户需要更多解释时打开主窗口；现有额度总览仍是默认入口，本地用量主窗口的可见信息架构
  在后续单独讨论前不预留空导航。
- 设置是主窗口内的二级视图；不创建独立窗口，也不为它增加常驻侧边栏或 Tab。
- 首次启动只完成最小状态检查、权限说明和凭据发现结果反馈。
- 应用在后台按用户设置自动刷新；用户也可以主动刷新。
- 网络失败、429、凭据异常和协议异常必须被准确区分。
- 开发与验收期间，CC Trace 与 cc-bar 可以在同一台机器上独立安装和运行。

## Capabilities and Constraints

- 首版 Provider 仅包含当前自动发现的一个 Codex 身份和一个 Claude Code 身份。
- 首版展示主要额度、必要专项额度、剩余百分比、重置时间和最近成功刷新时间。
- 每个 Provider 独立刷新、节流、退避和恢复，一个 Provider 的失败不能阻断另一个。
- 最新有效快照保存在 CC Trace 自己的缓存中；成功刷新产生的整数额度变化另存为按身份隔离的历史事件。
- 首版只读扫描 Codex 与 Claude Code 的本地 JSONL，标准化 Token 并在 CC Trace 自己的 SQLite 中建立可恢复索引。
- 价格目录是公开小表；费用是按模型、速度和推理地域估算的 API 等值金额，不是订阅实付金额，未知价格返回未定价。
- 紧凑入口按 Provider 展示今日／本周 API 等值费用；存在已定价记录时展示当前可计算金额，
  不追加金额下限符号或未定价说明；全部无法定价、未扫描或读取失败时显示占位。读数下方只
  显示“花费”。
- 本地用量在完成引导后立即执行首次扫描，之后每 5 分钟后台增量扫描一次；额度刷新不触发
  扫描。扫描进行时只在今日／本周费用末尾显示小号灰色 loading 状态。
- 界面必须覆盖 `docs/状态与错误模型.md` 定义的全部状态维度与失败原因，包括 `no_credentials`、`unsupported`、`offline`、`rate_limited` 与 `error`。
- 基础设置包括语言、外观、自动刷新间隔、开机启动和版本信息。
- 产品使用 Tauri 2、Vue 3、TypeScript、Rust、Pinia、Vue Router 与 Vue I18n。
- 系统能力、Provider 请求、凭据读取、调度和持久化留在 Rust；Vue 不取得秘密或通用系统权限。
- 首版本地对话只保存身份与聚合元数据，不保存或返回原始消息正文；不包含其他账号、桌面悬浮窗和应用内 Provider 登录。
- 第 6～8 阶段确认前不实现正式业务页面；原型确认前不实现 Tray 桌面壳；双平台桌面壳验证前不开始 Provider 最小闭环。

## Brand Commitments

- 正式产品名为 CC Trace，应用标识为 `com.nanvon.cctrace`。
- CC Trace 使用独立品牌，不复刻 Swift 版 cc-bar 的页面和视觉。
- 已确认的品牌符号为无尾巴双 `C`：两个外层 `C` 代表 Codex 与 Claude，内部使用同心的长短圆弧形成左向凝视。
- Logo、字标、App 图标以及 Tray／Menu Bar 微型图标均由 `design/brand/` 中的品牌母版派生。
- 产品表达应优先准确、快速、克制和可信，不使用旧数据伪装成功，也不通过夸张文案掩盖状态。

## Evidence on Hand

- 产品定位与体验原则：`docs/产品定义.md`
- 首版能力、非目标和验收标准：`docs/产品范围.md`
- 分层、安全、状态和实施边界：`docs/技术架构.md`
- 阶段门禁与执行状态：`docs/Tauri桌面端重新开发执行清单.md`
- 品牌几何和跨平台图标：`design/brand/`
- Swift 版 cc-bar 的只读业务事实：`docs/cc-bar-reference/`
- 未来脱敏测试样本边界：`fixtures/README.md`

当前没有可用于正式 Provider 实现的真实凭据、真实账号信息或可提交的 Provider 响应样本。未来设计和实现不得虚构客户、使用数据、性能指标或 Provider 保证。

## Product Principles

1. **风险优先：** 最紧张的额度和异常状态优先于完整数据陈列。
2. **快速可读：** 紧凑入口无需进一步导航即可完成主要判断。
3. **状态诚实：** 实时数据、旧快照、离线和错误必须明确区分。
4. **平台得体：** 核心任务和业务语义一致，系统入口与窗口行为按 macOS、Windows 分别适配。
5. **权限克制：** 只申请首版能力真正需要的权限，秘密不进入 Vue、普通日志或额度缓存。

## Accessibility & Inclusion

- 支持中文、英文和跟随系统语言。
- 支持浅色、深色和跟随系统外观。
- 核心流程必须支持键盘和明确焦点路径。
- 状态不能只依赖颜色表达，必须同时提供文字或图形语义。
- 动效遵循 reduced motion。
- 信息架构和原型必须验证中英文、长账号名、长模型名及窗口尺寸变化。

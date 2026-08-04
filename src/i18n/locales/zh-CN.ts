/**
 * 简体中文文案。
 *
 * 命名空间与规则见 `docs/文案与国际化.md` 第 2 节：按表面分组、语义命名、一条完整
 * 句子一个 key、不拼接句子。`preview.*` 是桌面壳合成数据专用，Provider 接入时整块删除。
 */
export default {
  common: {
    refresh: "刷新",
    settings: "设置",
    quit: "退出",
    details: "查看详情",
    close: "关闭",
    retry: "重试",
  },

  provider: {
    codex: "Codex",
    claude: "Claude Code",
    plan: "{plan} 计划",
    account: "账号 {hint}",
  },

  quota: {
    window: {
      fiveHour: "5 小时窗口",
      weekly: "每周窗口",
      modelWeekly: "{model} 每周窗口",
      unknown: "额度窗口",
    },
    resetLabel: "重置",
    /** 视觉上是紧凑倒计时；这条完整说法留给 `title` 与无障碍名称，参数是绝对时刻。 */
    resetsAt: "{time} 重置",
    resetsUnknown: "重置时间未知",
    retryIn: "{time}可再试",
    refreshedAgo: "{time} 前已刷新",
    refreshedJustNow: "刚刚已刷新",
    neverRefreshed: "尚未成功刷新",
    noValue: "—",
  },

  status: {
    loading: "正在检查额度",
    refreshing: "正在刷新，显示上一份数据",
    live: "额度状态正常",
    stale: "显示的是旧数据",
    noCredentials: "未发现可用凭据",
    unsupported: "凭据形式暂不支持",
    offlineStale: "当前离线，显示旧数据",
    offlineEmpty: "当前离线，没有可显示的额度",
    rateLimited: "刷新受限",
    errorCredentials: "凭据已失效",
    errorProtocol: "无法读取额度数据",
  },

  impact: {
    stale: "当前显示 {time}的数据，不是最新额度。",
    empty: "这个 Provider 现在没有可显示的额度。",
    noCredentials: "CC Trace 只读取本机已有凭据，不会要求你粘贴 Token。",
    unsupported: "这个 Provider 的额度暂时无法读取。",
    rateLimited: "Provider 暂时限制了刷新频率，已保留上一份数据。",
  },

  nextStep: {
    stale: "刷新以获取最新额度",
    noCredentials: "在对应 CLI 登录后刷新",
    unsupported: "首版只支持自动发现的 OAuth 凭据",
    offlineStale: "网络恢复后刷新",
    offlineEmpty: "检查网络后重试",
    rateLimited: "冷却结束后可再次刷新",
    errorCredentials: "在对应 CLI 重新登录",
    errorProtocol: "稍后重试；持续出现请更新 CC Trace",
  },

  time: {
    justNow: "刚刚",
    underOneMinute: "不到 1 分钟",
  },

  compact: {
    /** 品牌表面名；不翻译。 */
    title: "CC Trace",
    signal: {
      live: "数据是最新的",
      stale: "部分数据已过期",
      attention: "有需要处理的问题",
      idle: "尚未连接",
    },
    usage: {
      today: "今日",
      thisWeek: "本周",
      amountExact: "{period} API 等值费用为 {amount}，不代表订阅实付金额。",
      amountPending: "{period}费用尚未完成本地索引。",
      amountUnpriced: "{period}用量暂无可匹配的公开价格。",
      scanScanning: "正在扫描本地 Token 用量",
      scanComplete: "本地 Token 用量已更新",
      scanPartial: "本地 Token 用量已更新，但部分数据未能读取",
      scanUnavailable: "暂时无法读取本地 Token 用量",
    },
  },

  main: {
    title: "用量",
    lastScan: "上次扫描 {time}",
    neverScanned: "尚未扫描本地用量",
    filter: "时间范围",
    customRange: "自定义日期范围",
    overview: "总览",
    totalTokens: "总 Token",
    tokenUnit: "Token",
    totalCost: "总花费",
    from: "起始",
    to: "结束",
    chartContext: "近 14 天 · 高亮为所选范围",
    byProvider: "按 Provider",
    dailyCost: "每日花费",
    byModel: "按模型",
    input: "输入",
    output: "输出",
    cacheHit: "缓存命中",
    cacheWrite: "缓存写入",
    cacheHitRate: "缓存命中率",
    total: "总 Token",
    cost: "花费",
    lessThanCent: "<$0.01",
    model: "模型",
    subtotal: "小计",
    totalModels: "合计 · {count} 个模型",
    unpriced: "未定价",
    empty: "所选时间范围内没有本地用量",
    loading: "正在读取本地用量",
    partial: "本地用量已更新，但部分数据未能读取",
    unavailable: "暂时无法读取本地用量",
    noValue: "—",
    range: {
      today: "今天",
      yesterday: "昨天",
      thisWeek: "本周",
      thisMonth: "本月",
      thisYear: "本年",
      last7Days: "7 天",
      last30Days: "30 天",
      all: "全部",
    },
  },

  settings: {
    title: "设置",
    backToUsage: "返回用量",
    general: "通用",
    refreshInterval: "自动刷新间隔",
    launchAtLogin: "开机时启动 CC Trace",
    appearanceAndLanguage: "外观与语言",
    language: "语言",
    appearance: "外观",
    usageAndPricing: "用量与计价",
    pricingCatalog: "价格目录",
    pricingCatalogDescription: "立即检查 LiteLLM 与 models.dev；更新失败时继续使用原有价格。",
    pricingCatalogUpdate: "更新",
    pricingCatalogUpdating: "更新中…",
    pricingCatalogUpdated: "价格目录已是最新。",
    pricingCatalogPartiallyUpdated: "部分价格已更新；其余价格继续使用原有目录。",
    pricingCatalogUpdateFailed: "价格更新失败，当前仍使用原有目录。",
    about: "关于",
    version: "版本 {version}",
    privacy:
      "CC Trace 只读取本机已有的 Codex 与 Claude Code 凭据，不上传任何数据，也不读取或迁移 cc-bar 的应用数据。",
    intervalOption: {
      m1: "1 分钟",
      m2: "2 分钟",
      m3: "3 分钟",
      m5: "5 分钟",
      m10: "10 分钟",
    },
    languageOption: {
      system: "跟随系统",
      chinese: "中文",
      english: "English",
    },
    appearanceOption: {
      system: "跟随系统",
      light: "浅色",
      dark: "深色",
    },
  },

  onboarding: {
    title: "认识 CC Trace",
    intro: "CC Trace 让你随时看到 Codex 与 Claude Code 还剩多少额度、什么时候重置。",
    residency: "它常驻在系统区域。点击图标打开紧凑面板，需要细节时再打开主窗口。",
    checkHeading: "本机状态",
    checkNow: "检查本机",
    checkAgain: "重新检查",
    checking: "检查中…",
    notChecked: "等待检查",
    keychainNotice:
      "macOS 首次访问 Claude Code 凭据时可能请求钥匙串授权；选择“始终允许”可避免当前版本重复询问。",
    boundaryHeading: "数据边界",
    boundary:
      "凭据只在本机按最小范围访问；仅 token 续期结果原位回写。数据不上传，也不读取或迁移 cc-bar 数据。",
    noCredentialsHint: "现在没有凭据也可以继续。在对应 CLI 登录后刷新即可。",
    done: "开始使用",
    later: "稍后再说",
  },

  error: {
    settingsWriteFailed: {
      title: "设置没有保存",
      nextStep: "已保留原来的选项，稍后重试",
    },
  },

  a11y: {
    refreshAll: "刷新全部额度",
    quotaRail: "{provider} {window}",
    noQuota: "无可用额度",
    remaining: "剩余 {percent}",
    closePanel: "关闭面板",
    statusRegion: "额度状态",
    apiEquivalentCosts: "{provider} 今日与本周 API 等值费用",
    usageRegion: "本地用量",
    usageChart: "按 Provider 的每日花费图",
    usageTable: "按模型的用量明细",
    sortColumn: "按 {column}排序",
  },

  preview: {
    badge: "合成数据",
    notice: "当前数值不来自 Codex 或 Claude Code，只用于验证桌面壳与状态表达。",
    scenario: "验证场景",
    scenarioOption: {
      live: "真实数据",
      healthy: "两个都正常",
      firstLoad: "首次加载",
      noCredentials: "无凭据",
      unsupported: "凭据不支持",
      offlineStale: "离线 · 有旧快照",
      offlineEmpty: "离线 · 无快照",
      rateLimited: "429 · 故障隔离",
      errorStale: "凭据失效 · 有旧快照",
      errorEmpty: "协议错误 · 无快照",
    },
  },
};

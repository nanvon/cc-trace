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
    resetsAt: "{time} 重置",
    resetsUnknown: "重置时间未知",
    retryIn: "{time}可再试",
    lastSuccess: "上次成功刷新 {time}",
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
    title: "用量",
    signal: {
      live: "数据是最新的",
      stale: "部分数据已过期",
      attention: "有需要处理的问题",
      idle: "尚未连接",
    },
  },

  main: {
    title: "额度总览",
  },

  settings: {
    title: "设置",
    backToQuota: "返回额度",
    general: "通用",
    refreshInterval: "自动刷新间隔",
    launchAtLogin: "开机时启动 CC Trace",
    appearanceAndLanguage: "外观与语言",
    language: "语言",
    appearance: "外观",
    about: "关于",
    version: "版本 {version}",
    privacy:
      "CC Trace 只读取本机已有的 Codex 与 Claude Code 凭据，不上传任何数据，也不读取或迁移 cc-bar 的应用数据。",
    intervalOption: {
      m15: "15 分钟",
      m30: "30 分钟",
      m60: "60 分钟",
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
    boundaryHeading: "数据边界",
    boundary:
      "凭据只在本机由 Rust 层按只读、最小范围发现，不上传，也不会读取或迁移 cc-bar 的任何数据。",
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

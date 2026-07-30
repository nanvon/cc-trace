/**
 * English copy. Sentence case throughout; no all-caps labels beyond the confirmed
 * short utility ones. Keys mirror `zh-CN.ts` exactly — both languages ship together.
 */
export default {
  common: {
    refresh: "Refresh",
    settings: "Settings",
    quit: "Quit",
    details: "View details",
    close: "Close",
    retry: "Retry",
  },

  provider: {
    codex: "Codex",
    claude: "Claude Code",
    plan: "{plan} plan",
    account: "Account {hint}",
  },

  quota: {
    window: {
      fiveHour: "5-hour window",
      weekly: "Weekly window",
      modelWeekly: "{model} weekly window",
      unknown: "Quota window",
    },
    resetLabel: "reset",
    /** Shown as a compact countdown; this full wording is for `title` and a11y names. */
    resetsAt: "Resets {time}",
    resetsUnknown: "Reset time unknown",
    retryIn: "Retry {time}",
    refreshedAgo: "refreshed {time} ago",
    refreshedJustNow: "refreshed just now",
    neverRefreshed: "No successful refresh yet",
    noValue: "—",
  },

  status: {
    loading: "Checking quota",
    refreshing: "Refreshing, showing last data",
    live: "Quota is healthy",
    stale: "Showing older data",
    noCredentials: "No credentials found",
    unsupported: "Credential type not supported",
    offlineStale: "Offline, showing older data",
    offlineEmpty: "Offline, no quota to show",
    rateLimited: "Refresh rate-limited",
    errorCredentials: "Credentials are no longer valid",
    errorProtocol: "Could not read quota data",
  },

  impact: {
    stale: "Showing data from {time}, not the current quota.",
    empty: "There is no quota to show for this provider.",
    noCredentials:
      "CC Trace only reads credentials already on this machine and never asks you to paste a token.",
    unsupported: "This provider's quota cannot be read right now.",
    rateLimited: "The provider is limiting refreshes; your last data is kept.",
  },

  nextStep: {
    stale: "Refresh to get the latest quota",
    noCredentials: "Sign in with the CLI, then refresh",
    unsupported: "This release supports auto-discovered OAuth credentials only",
    offlineStale: "Refresh once you are back online",
    offlineEmpty: "Check your connection and retry",
    rateLimited: "You can refresh again after the cooldown",
    errorCredentials: "Sign in again with the CLI",
    errorProtocol: "Try again later; update CC Trace if it persists",
  },

  time: {
    justNow: "just now",
    underOneMinute: "under a minute",
  },

  compact: {
    title: "Usage",
    signal: {
      live: "Data is current",
      stale: "Some data is stale",
      attention: "Something needs attention",
      idle: "Not connected yet",
    },
    usage: {
      today: "today",
      thisWeek: "this week",
      costLabel: "Cost",
      amountExact:
        "{period} API-equivalent cost is {amount}. This is not the amount paid for a subscription.",
      amountPending: "The {period} cost has not been indexed yet.",
      amountUnpriced: "No public price matches the {period} usage.",
      scanScanning: "Scanning local Token usage",
      scanComplete: "Local Token usage updated",
      scanPartial: "Local Token usage updated, but some data could not be read",
      scanUnavailable: "Local Token usage is temporarily unavailable",
    },
  },

  main: {
    title: "Quota overview",
  },

  settings: {
    title: "Settings",
    backToQuota: "Back to quota",
    general: "General",
    refreshInterval: "Auto-refresh interval",
    launchAtLogin: "Launch CC Trace at login",
    appearanceAndLanguage: "Appearance & language",
    language: "Language",
    appearance: "Appearance",
    usageAndPricing: "Usage & pricing",
    pricingCatalog: "Pricing catalog",
    pricingCatalogDescription:
      "Check LiteLLM and models.dev now. Existing prices stay available if the update fails.",
    pricingCatalogUpdate: "Update",
    pricingCatalogUpdating: "Updating…",
    pricingCatalogUpdated: "Pricing catalog is up to date.",
    pricingCatalogPartiallyUpdated:
      "Some prices were updated; the previous catalog remains in use for the rest.",
    pricingCatalogUpdateFailed: "Could not update prices. The previous catalog is still in use.",
    about: "About",
    version: "Version {version}",
    privacy:
      "CC Trace only reads the Codex and Claude Code credentials already on this machine. It uploads nothing, and it never reads or migrates cc-bar application data.",
    intervalOption: {
      m15: "15 minutes",
      m30: "30 minutes",
      m60: "60 minutes",
    },
    languageOption: {
      system: "Follow system",
      chinese: "中文",
      english: "English",
    },
    appearanceOption: {
      system: "Follow system",
      light: "Light",
      dark: "Dark",
    },
  },

  onboarding: {
    title: "Meet CC Trace",
    intro: "CC Trace shows how much Codex and Claude Code quota you have left, and when it resets.",
    residency:
      "It lives in the system area. Click the icon for the compact panel, and open the main window when you need detail.",
    checkHeading: "This machine",
    checkNow: "Check this machine",
    checkAgain: "Check again",
    checking: "Checking…",
    notChecked: "Not checked yet",
    keychainNotice:
      "macOS may request Keychain access the first time CC Trace checks Claude Code. Choose “Always Allow” to avoid repeated prompts for this version.",
    boundaryHeading: "Data boundary",
    boundary:
      "Credentials are accessed locally and minimally; only renewed tokens are written back in place. Nothing is uploaded, and no cc-bar data is read or migrated.",
    noCredentialsHint:
      "You can continue without credentials. Sign in with the CLI and refresh whenever you are ready.",
    done: "Start using CC Trace",
    later: "Not now",
  },

  error: {
    settingsWriteFailed: {
      title: "Settings were not saved",
      nextStep: "Your previous choice is kept; try again in a moment",
    },
  },

  a11y: {
    refreshAll: "Refresh all quota",
    quotaRail: "{provider} {window}",
    noQuota: "No quota available",
    remaining: "{percent} remaining",
    closePanel: "Close panel",
    statusRegion: "Quota status",
    apiEquivalentCosts: "{provider} API-equivalent cost for today and this week",
  },

  preview: {
    badge: "Synthetic data",
    notice:
      "These numbers do not come from Codex or Claude Code. They exist to verify the desktop shell and its status states.",
    scenario: "Verification scenario",
    scenarioOption: {
      live: "Live data",
      healthy: "Both healthy",
      firstLoad: "First load",
      noCredentials: "No credentials",
      unsupported: "Unsupported credential",
      offlineStale: "Offline, has snapshot",
      offlineEmpty: "Offline, no snapshot",
      rateLimited: "429, fault isolation",
      errorStale: "Bad credentials, has snapshot",
      errorEmpty: "Protocol error, no snapshot",
    },
  },
};

/**
 * 应用偏好契约。与 `src-tauri/src/contracts/settings.rs` 一一对应。
 * 候选值与默认值由 `docs/产品范围.md`「基础设置」拥有。
 */

export type LanguagePreference = "system" | "zh-CN" | "en";
export type AppearancePreference = "system" | "light" | "dark";

/** 首版不提供关闭自动刷新，因此没有 `off` 取值。 */
export type RefreshIntervalOption = "1m" | "2m" | "3m" | "5m" | "10m";

export interface OnboardingState {
  completed: boolean;
  completedAt: string | null;
}

/** 计入统计的数据源；Antigravity 与 Command Code 只提供额度，没有本地用量。 */
export type StatsServiceSource = "codex" | "claude" | "pi" | "opencode" | "dsh" | "cursor";

/** 单个服务的四组开关。四组互相独立。 */
export interface ServiceSettings {
  quota: boolean;
  menuBar: boolean;
  hud: boolean;
  stats: boolean;
}

export interface ServicesSettings {
  codex: ServiceSettings;
  claude: ServiceSettings;
  antigravity: ServiceSettings;
  cursor: ServiceSettings;
  commandCode: ServiceSettings;
  /** Pi／OpenCode／DSH 三个无额度数据源共用统计开关。 */
  localAgentStats: boolean;
}

/** 排行口径：用量构成、高消耗对话、项目与对话列表的默认排序依据。 */
export type RankingBasis = "tokens" | "cost";

/** 重置时间显示方式。 */
export type ResetTimeDisplay = "duration" | "dateTime";

/** 系统区域承载哪个额度窗口。 */
export type MenuBarWindowMode = "primary" | "weekly" | "both";

/** Command Code 凭据读取偏好：自动按顺序尝试，或只看手动填写的 API Key。 */
export type CommandCodeCredentialPreference = "automatic" | "manual";

/** Command Code 的凭据来源，用于说明「当前用的是哪一个」。 */
export type CommandCodeCredentialSource = "commandcode" | "pi" | "opencode" | "env" | "keychain";

/** 凭据管理命令的返回值；不回传 Key 本身。 */
export interface CommandCodeCredentialState {
  preference: CommandCodeCredentialPreference;
  hasManualKey: boolean;
}

export interface HudPosition {
  x: number;
  y: number;
}

export interface HudSettings {
  enabled: boolean;
  position: HudPosition | null;
}

export interface Settings {
  schemaVersion: number;
  language: LanguagePreference;
  appearance: AppearancePreference;
  /** 额度自动刷新间隔。 */
  refreshInterval: RefreshIntervalOption;
  /** 本地日志扫描间隔；与额度间隔相互独立。 */
  scanInterval: RefreshIntervalOption;
  launchAtLogin: boolean;
  privacyMode: boolean;
  /** 服务状态圆点：只控制紧凑面板绘制，后台拉取不受影响（ADR-0026）。 */
  showServiceStatus: boolean;
  menuBarWindowMode: MenuBarWindowMode;
  commandCodeCredential: CommandCodeCredentialPreference;
  services: ServicesSettings;
  hud: HudSettings;
  rankingBasis: RankingBasis;
  resetTimeDisplay: ResetTimeDisplay;
  checkUpdatesOnStart: boolean;
  verboseLogging: boolean;
  onboarding: OnboardingState;
}

/** 部分更新。省略的字段保持原值。 */
export interface SettingsUpdate {
  language?: LanguagePreference;
  appearance?: AppearancePreference;
  refreshInterval?: RefreshIntervalOption;
  scanInterval?: RefreshIntervalOption;
  launchAtLogin?: boolean;
  privacyMode?: boolean;
  showServiceStatus?: boolean;
  menuBarWindowMode?: MenuBarWindowMode;
  commandCodeCredential?: CommandCodeCredentialPreference;
  services?: ServicesSettings;
  hud?: HudSettings;
  rankingBasis?: RankingBasis;
  resetTimeDisplay?: ResetTimeDisplay;
  checkUpdatesOnStart?: boolean;
  verboseLogging?: boolean;
}

export const RANKING_BASIS_OPTIONS: readonly RankingBasis[] = ["tokens", "cost"] as const;
export const RESET_TIME_DISPLAY_OPTIONS: readonly ResetTimeDisplay[] = [
  "duration",
  "dateTime",
] as const;
export const MENU_BAR_WINDOW_MODE_OPTIONS: readonly MenuBarWindowMode[] = [
  "primary",
  "weekly",
  "both",
] as const;

export const LANGUAGE_OPTIONS: readonly LanguagePreference[] = ["system", "zh-CN", "en"] as const;
export const APPEARANCE_OPTIONS: readonly AppearancePreference[] = [
  "system",
  "light",
  "dark",
] as const;
export const REFRESH_INTERVAL_OPTIONS: readonly RefreshIntervalOption[] = [
  "1m",
  "2m",
  "3m",
  "5m",
  "10m",
] as const;

export interface AppStatus {
  name: string;
  version: string;
  platform: string;
  /** 由 Rust 平台层解析，前端不读 `navigator.language`。 */
  systemLocale: string;
}

/** 命令失败的稳定标识，由界面查 i18n 文案。 */
export interface CommandError {
  code: "windowUnavailable" | "settingsWriteFailed";
}

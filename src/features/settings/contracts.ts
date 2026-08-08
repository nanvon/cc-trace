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

/** 统计服务可见性：关闭的服务从本地用量统计统一过滤。默认全开。 */
export type StatsServiceSource = "codex" | "claude" | "pi" | "opencode";

export interface UsageServiceVisibility {
  codex: boolean;
  claude: boolean;
  pi: boolean;
  opencode: boolean;
}

export interface Settings {
  schemaVersion: number;
  language: LanguagePreference;
  appearance: AppearancePreference;
  refreshInterval: RefreshIntervalOption;
  launchAtLogin: boolean;
  onboarding: OnboardingState;
  usageServiceVisibility: UsageServiceVisibility;
}

/** 部分更新。省略的字段保持原值。 */
export interface SettingsUpdate {
  language?: LanguagePreference;
  appearance?: AppearancePreference;
  refreshInterval?: RefreshIntervalOption;
  launchAtLogin?: boolean;
  usageServiceVisibility?: UsageServiceVisibility;
}

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

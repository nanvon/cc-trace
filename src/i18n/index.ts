import { createI18n } from "vue-i18n";

import type { AppearancePreference, LanguagePreference } from "../features/settings/contracts";
import en from "./locales/en";
import zhCN from "./locales/zh-CN";

export type AppLocale = "zh-CN" | "en";

/**
 * 启动时先用回退语言，等 Rust 下发的设置与系统语言到达后立刻修正。
 * 这里**不读** `navigator.language`：语言判定只有 `resolveLocale` 一处，
 * 见 `docs/文案与国际化.md` 第 1 节。
 */
export const i18n = createI18n({
  legacy: false,
  locale: "en",
  fallbackLocale: "en",
  messages: {
    "zh-CN": zhCN,
    en,
  },
});

export function resolveLocale(preference: LanguagePreference, systemLocale: string): AppLocale {
  if (preference === "zh-CN" || preference === "en") {
    return preference;
  }
  return systemLocale.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

/** 切换界面语言，并同步 `<html lang>` 供辅助技术与断词使用。 */
export function applyLocale(locale: AppLocale): void {
  // legacy: false 下 `locale` 是 WritableComputedRef，但导出类型随配置推断而变，
  // 这里用最窄的结构断言，避免把整个 i18n 实例的泛型摊开。
  (i18n.global.locale as unknown as { value: AppLocale }).value = locale;
  document.documentElement.lang = locale;
}

/** 外观偏好写到根元素，由 `tokens.css` 决定「跟随系统」还是强制浅色 / 深色。 */
export function applyAppearance(preference: AppearancePreference): void {
  document.documentElement.dataset.appearance = preference;
}

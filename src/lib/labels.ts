/**
 * 额度窗口与 Provider 的展示名。
 *
 * 窗口名按 `kind` 取 i18n 文案；`modelWeekly` 才用 Provider 派生的 `displayName`
 * 作为参数。这样同一个语义窗口在两种语言下只有一处译法。
 */

import type { ComposerTranslation } from "vue-i18n";

import type { ProviderId, QuotaWindow } from "../features/quota/contracts";

export function providerLabel(t: ComposerTranslation, provider: ProviderId): string {
  return t(`provider.${provider}`);
}

export function windowLabel(t: ComposerTranslation, window: QuotaWindow): string {
  if (window.kind === "modelWeekly" && window.displayName) {
    return t("quota.window.modelWeekly", { model: window.displayName });
  }
  return t(`quota.window.${window.kind}`);
}

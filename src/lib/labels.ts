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

/**
 * 套餐名的展示格式。
 *
 * Provider 凭据里的套餐值是外部系统的原文（Codex 的 `chatgpt_plan_type` 全小写，如
 * `"plus"` `"pro"`），不应该在解析层臆造格式；这里只是展示层的大小写规范化，
 * 按单词首字母大写，不改变原始语义。
 */
export function planLabel(plan: string): string {
  return plan
    .split(/([\s_-]+)/)
    .map((part) => (/[\s_-]/.test(part) ? part : part.charAt(0).toUpperCase() + part.slice(1)))
    .join("");
}

export function windowLabel(t: ComposerTranslation, window: QuotaWindow): string {
  if (window.kind === "modelWeekly" && window.displayName) {
    return t("quota.window.modelWeekly", { model: window.displayName });
  }
  return t(`quota.window.${window.kind}`);
}

/**
 * 读数位上的窗口短码：`5HOUR`、`WEEKLY`、`ALL`、`OPUS`。
 *
 * 刻意语言中立，两种界面语言下都是大写拉丁短码（ADR-0019）。它是定宽读数行的一部分，
 * 中文长句「Opus 每周窗口」在 380px 面板里放不下。完整窗口名仍由 `windowLabel`
 * 提供给 `title` 与无障碍名称，因此短码不是这个信息的唯一载体。
 *
 * Claude Code 的周窗口是全模型合计，读作 `ALL`；Codex 的周窗口就是周窗口。
 */
export function windowCode(provider: ProviderId, window: QuotaWindow): string {
  switch (window.kind) {
    case "fiveHour":
      return "5HOUR";
    case "weekly":
      return provider === "claude" ? "ALL" : "WEEKLY";
    case "modelWeekly":
      return window.displayName?.toUpperCase() ?? "MODEL";
    default:
      return window.displayName?.toUpperCase() ?? "CURRENT";
  }
}

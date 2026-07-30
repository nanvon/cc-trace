/**
 * 把 `format.ts` 的计算结果接上 i18n 词语。
 *
 * `format.ts` 只返回分支和 Intl 结果，词语（「刚刚」「不到 1 分钟」）留在文案文件里，
 * 因此同一个说法在两种语言下只有一处译法。
 *
 * 紧凑读数（`reset`、`refreshed`）依赖 `useNow()`，因此调用方的 `computed` 会随
 * 分钟推进自动重算，不需要各自管计时器。
 */

import { useI18n } from "vue-i18n";

import { compactAge, compactReset, formatCountdown, formatPast } from "./format";
import { useNow } from "./useNow";

export function useTimeText() {
  const { t, locale } = useI18n();
  const now = useNow();

  /** 过去时刻的相对描述，用于提示条里的解释句。`null` 表示还没有成功过。 */
  function past(iso: string | null): string {
    if (!iso) {
      return t("quota.neverRefreshed");
    }
    const result = formatPast(locale.value, iso, now.value);
    if (result.kind === "justNow") {
      return t("time.justNow");
    }
    if (result.kind === "underOneMinute") {
      return t("time.underOneMinute");
    }
    return result.text;
  }

  /** 未来时刻的倒计时，精度到分钟。 */
  function countdown(iso: string | null): string | null {
    if (!iso) {
      return null;
    }
    const result = formatCountdown(locale.value, iso, now.value);
    if (result.kind === "justNow") {
      return t("time.justNow");
    }
    if (result.kind === "underOneMinute") {
      return t("time.underOneMinute");
    }
    return result.text;
  }

  /**
   * 重置倒计时的定宽读数。`resetsAt` 缺失时给占位符——它旁边的「重置」标签仍在，
   * 语义不丢，完整说明由 `title` 承担。
   */
  function reset(iso: string | null): string {
    return iso ? compactReset(iso, now.value) : t("quota.noValue");
  }

  /** 头部副标题：紧凑读数 + 「前已刷新」。 */
  function refreshed(iso: string | null): string {
    if (!iso) {
      return t("quota.neverRefreshed");
    }
    const result = compactAge(iso, now.value);
    return result.kind === "justNow"
      ? t("quota.refreshedJustNow")
      : t("quota.refreshedAgo", { time: result.text });
  }

  return { past, countdown, reset, refreshed };
}

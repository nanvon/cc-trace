/**
 * 把 `format.ts` 的计算结果接上 i18n 词语。
 *
 * `format.ts` 只返回分支和 Intl 结果，词语（「刚刚」「不到 1 分钟」）留在文案文件里，
 * 因此同一个说法在两种语言下只有一处译法。
 */

import { useI18n } from "vue-i18n";

import { formatCountdown, formatPast } from "./format";

export function useTimeText() {
  const { t, locale } = useI18n();

  /** 过去时刻的相对描述。`null` 表示还没有成功过。 */
  function past(iso: string | null): string {
    if (!iso) {
      return t("quota.neverRefreshed");
    }
    const result = formatPast(locale.value, iso);
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
    const result = formatCountdown(locale.value, iso);
    if (result.kind === "justNow") {
      return t("time.justNow");
    }
    if (result.kind === "underOneMinute") {
      return t("time.underOneMinute");
    }
    return result.text;
  }

  return { past, countdown };
}

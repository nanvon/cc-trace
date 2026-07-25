/**
 * 时间与数字格式化。
 *
 * 规则由 `docs/文案与国际化.md` 第 3 节拥有：
 * - Rust 下发的所有时刻都是 ISO 8601 UTC，展示层负责本地化。
 * - 一律用 `Intl.*`，locale 取当前界面语言，不硬编码格式。
 * - 界面显示本地时区，不显示 UTC 标记。
 *
 * 本模块只做计算，不含任何可见文案：需要词语的地方返回 `kind`，由调用方查 i18n。
 */

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/** 相对时间的展示分支。`justNow` 与 `underOneMinute` 的词语由 i18n 提供。 */
export type RelativeTime =
  | { kind: "justNow" }
  | { kind: "underOneMinute" }
  | { kind: "relative"; text: string }
  | { kind: "absolute"; text: string };

/**
 * 剩余百分比。整数四舍五入；大于 0 但不足 1 显示 `<1%`，避免读成「没有额度」。
 */
export function formatPercent(locale: string, value: number): string {
  if (value > 0 && value < 1) {
    return "<1%";
  }
  return new Intl.NumberFormat(locale, {
    style: "percent",
    maximumFractionDigits: 0,
  }).format(value / 100);
}

/**
 * 重置时刻的主显示：当天只给时:分，跨天补上日期。
 */
export function formatResetClock(locale: string, iso: string, now: Date = new Date()): string {
  const date = new Date(iso);
  const sameDay = date.toDateString() === now.toDateString();

  return new Intl.DateTimeFormat(locale, {
    ...(sameDay ? {} : { month: "short", day: "numeric" }),
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

/**
 * 完整本地时刻。相对时间必须同时通过 `title` 或 `aria-label` 暴露它。
 */
export function formatAbsolute(locale: string, iso: string): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(iso));
}

/**
 * 过去时刻的相对描述。超过 24 小时改用绝对日期，不再说「28 小时前」。
 */
export function formatPast(locale: string, iso: string, now: Date = new Date()): RelativeTime {
  const elapsed = now.getTime() - new Date(iso).getTime();

  if (elapsed < MINUTE_MS) {
    return { kind: "justNow" };
  }
  if (elapsed >= DAY_MS) {
    return {
      kind: "absolute",
      text: new Intl.DateTimeFormat(locale, { month: "short", day: "numeric" }).format(
        new Date(iso),
      ),
    };
  }

  const relative = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const text =
    elapsed < HOUR_MS
      ? relative.format(-Math.round(elapsed / MINUTE_MS), "minute")
      : relative.format(-Math.round(elapsed / HOUR_MS), "hour");

  return { kind: "relative", text };
}

/**
 * 未来时刻的倒计时，精度到分钟。不足 1 分钟不显示秒级跳字。
 */
export function formatCountdown(locale: string, iso: string, now: Date = new Date()): RelativeTime {
  const remaining = new Date(iso).getTime() - now.getTime();

  if (remaining <= 0) {
    return { kind: "justNow" };
  }
  if (remaining < MINUTE_MS) {
    return { kind: "underOneMinute" };
  }

  const relative = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const text =
    remaining < HOUR_MS
      ? relative.format(Math.round(remaining / MINUTE_MS), "minute")
      : relative.format(Math.round(remaining / HOUR_MS), "hour");

  return { kind: "relative", text };
}

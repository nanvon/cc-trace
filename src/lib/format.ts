/**
 * 时间与数字格式化。
 *
 * 规则由 `docs/文案与国际化.md` 第 3 节拥有：
 * - Rust 下发的所有时刻都是 ISO 8601 UTC，展示层负责本地化。
 * - 需要按语言变化的格式一律用 `Intl.*`，locale 取当前界面语言。
 * - 界面显示本地时区，不显示 UTC 标记。
 *
 * 例外是**紧凑时长**（`6d2h`、`1h42m`）：它语言中立，`Intl` 也产不出这种形态，
 * 见 ADR-0019。定宽是它存在的理由——380px 面板放不下会随日期变宽的读数。
 *
 * 本模块只做计算，不含任何可见文案：需要词语的地方返回 `kind`，由调用方查 i18n。
 */

const SECOND_MS = 1_000;
const MINUTE_MS = 60 * SECOND_MS;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const USD_NANOS = 1_000_000_000;

/** 相对时间的展示分支。`justNow` 与 `underOneMinute` 的词语由 i18n 提供。 */
export type RelativeTime =
  | { kind: "justNow" }
  | { kind: "underOneMinute" }
  | { kind: "relative"; text: string }
  | { kind: "absolute"; text: string };

/**
 * 紧凑时长的展示分支。`compact.text` 已经是可直接显示的语言中立读数，
 * 只有 `justNow` 需要调用方查词。
 *
 * 与 `RelativeTime` 的分工是信息层级，不是重复实现：定宽读数位用紧凑形态，
 * 提示条里的解释句用 `RelativeTime` 的自然语言（「5 分钟前的数据」）。
 */
export type CompactTime = { kind: "justNow" } | { kind: "compact"; text: string };

/**
 * 剩余百分比。整数四舍五入；大于 0 但不足 1 显示 `<1%`，避免读成「没有额度」。
 */
export function formatPercent(locale: string, value: number): string {
  const { value: number, unit } = splitPercent(locale, value);
  return `${number}${unit}`;
}

/**
 * 拆开的百分比，供大读数给数字和 `%` 设不同字号（ADR-0019）。
 *
 * 用 `formatToParts` 而不是切字符串，locale 自己决定分组符与 `%` 的写法。
 * 当前两种界面语言的 `%` 都在数字之后；新增 `%` 前置的语言时必须复核拼接顺序。
 */
export function splitPercent(locale: string, value: number): { value: string; unit: string } {
  if (value > 0 && value < 1) {
    return { value: "<1", unit: "%" };
  }

  const parts = new Intl.NumberFormat(locale, {
    style: "percent",
    maximumFractionDigits: 0,
  }).formatToParts(value / 100);

  const isUnit = (type: Intl.NumberFormatPart["type"]): boolean =>
    type === "percentSign" || type === "literal";

  return {
    value: parts
      .filter((part) => !isUnit(part.type))
      .map((part) => part.value)
      .join(""),
    unit: parts
      .filter((part) => isUnit(part.type))
      .map((part) => part.value)
      .join(""),
  };
}

/**
 * Popover 的定宽美元读数，与 cc-bar 相同：小于一美元显示 `<$1`，其余取整。
 * `$0` 只表示调用方已经确认的真实零值；未扫描、失败与全部未定价由调用方显示 `—`。
 */
export function formatCompactUsdNanos(locale: string, nanos: number): string {
  if (nanos <= 0) {
    return "$0";
  }
  if (nanos < USD_NANOS) {
    return "<$1";
  }

  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: "USD",
    currencyDisplay: "narrowSymbol",
    minimumFractionDigits: 0,
    maximumFractionDigits: 0,
  }).format(nanos / USD_NANOS);
}

/**
 * 紧凑 token 读数，口径与 cc-bar 一致：不足千原样，其余按界面语言取
 * Intl 紧凑记数法（en `67.7K`、zh-CN `6.8万`）。只用于图表刻度与 Tooltip。
 */
export function formatCompactTokens(locale: string, value: number): string {
  return new Intl.NumberFormat(locale, {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(Math.max(0, value));
}

/** 完整金额只用于 Tooltip 与无障碍说明，不受 popover 定宽取整限制。 */
export function formatUsdNanos(locale: string, nanos: number): string {
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: "USD",
    currencyDisplay: "narrowSymbol",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(Math.max(0, nanos) / USD_NANOS);
}

/**
 * 紧凑时长：`<1m`、`43m`、`4h37m`、`5d3h`。无空格、无单位词，最长 6 个字符（`23h59m`）。
 *
 * 只保留两个最高量级：距重置还有 6 天时，分钟数没有决策价值，但会让读数变宽。
 */
export function compactDuration(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) {
    return "<1m";
  }

  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m`;
  }

  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    const restMinutes = minutes % 60;
    return restMinutes > 0 ? `${hours}h${restMinutes}m` : `${hours}h`;
  }

  const days = Math.floor(hours / 24);
  const restHours = hours % 24;
  return restHours > 0 ? `${days}d${restHours}h` : `${days}d`;
}

/**
 * 重置倒计时的主显示。已经过去的时刻按 `<1m` 处理，不显示负数。
 */
export function compactReset(iso: string, now: Date = new Date()): string {
  return compactDuration(new Date(iso).getTime() - now.getTime());
}

/**
 * 距上次成功刷新的时长。1 分钟窗口内给秒级读数（`32s`），不足 1 秒仍交给 i18n 说
 * 「刚刚」；窗口外回到分钟级紧凑时长。
 *
 * 秒级只服务头部副标题这一处，且只存在 1 分钟：调用方在窗口内订阅秒级 tick
 * （`useNowSeconds`），窗口外切回分钟级 `useNow`，见 ADR-0019 修订。
 */
export function compactAge(iso: string, now: Date = new Date()): CompactTime {
  const elapsed = now.getTime() - new Date(iso).getTime();
  if (elapsed < SECOND_MS) {
    return { kind: "justNow" };
  }
  if (elapsed < MINUTE_MS) {
    return { kind: "compact", text: `${Math.floor(elapsed / SECOND_MS)}s` };
  }
  return { kind: "compact", text: compactDuration(elapsed) };
}

/**
 * 过去时刻的相对描述，用于提示条里的解释句。超过 24 小时改用绝对日期，
 * 不再说「28 小时前」。定宽读数位用 `compactAge`，不用这个。
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
 * 完整本地时刻。相对时间必须同时通过 `title` 或 `aria-label` 暴露它。
 */
export function formatAbsolute(locale: string, iso: string): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(iso));
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

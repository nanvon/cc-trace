import type { UsageDashboardRange, UsageGranularity } from "./contracts";

/** Popover 的两个固定时间范围；与 cc-bar 一样，本周从本地日历周一开始。 */
export interface UsageCostRanges {
  today: { from: string; to: string };
  week: { from: string; to: string };
}

export type UsageRangePreset = Exclude<UsageDashboardRange["preset"], "custom">;

/** 单周期上下文窗口的周期数（含所选那个）：日 30、周／月 14（cc-bar `contextWindowPeriods`）。 */
const CONTEXT_WINDOW_PERIODS: Record<UsageGranularity, number> = { day: 30, week: 14, month: 14 };

const PRESETS: UsageRangePreset[] = [
  "today",
  "yesterday",
  "thisWeek",
  "thisMonth",
  "thisYear",
  "last7Days",
  "last30Days",
  "all",
];

function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function toIso(date: Date): string {
  return date.toISOString();
}

function range(
  from: Date | null,
  to: Date | null,
  preset: UsageDashboardRange["preset"],
): UsageDashboardRange {
  return { preset, from: from ? toIso(from) : null, to: to ? toIso(to) : null };
}

/** 所有预设都使用本地日历边界，再交给 Rust 做 UTC 查询。 */
export function usageDashboardRanges(
  now: Date = new Date(),
): Record<UsageRangePreset, UsageDashboardRange> {
  const today = startOfDay(now);
  const tomorrow = new Date(today.getFullYear(), today.getMonth(), today.getDate() + 1);
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  const daysSinceMonday = (today.getDay() + 6) % 7;
  const monday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - daysSinceMonday);
  const month = new Date(today.getFullYear(), today.getMonth(), 1);
  const year = new Date(today.getFullYear(), 0, 1);
  const last7Days = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 6);
  const last30Days = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 29);
  const lastWeekStart = new Date(monday.getFullYear(), monday.getMonth(), monday.getDate() - 7);
  const last4WeeksStart = new Date(monday.getFullYear(), monday.getMonth(), monday.getDate() - 21);
  const last12WeeksStart = new Date(monday.getFullYear(), monday.getMonth(), monday.getDate() - 77);
  const lastMonthStart = new Date(today.getFullYear(), today.getMonth() - 1, 1);
  const last6MonthsStart = new Date(today.getFullYear(), today.getMonth() - 5, 1);

  return {
    today: range(today, tomorrow, "today"),
    yesterday: range(yesterday, today, "yesterday"),
    thisWeek: range(monday, tomorrow, "thisWeek"),
    thisMonth: range(month, tomorrow, "thisMonth"),
    thisYear: range(year, tomorrow, "thisYear"),
    last7Days: range(last7Days, tomorrow, "last7Days"),
    last30Days: range(last30Days, tomorrow, "last30Days"),
    lastWeek: range(lastWeekStart, monday, "lastWeek"),
    last4Weeks: range(last4WeeksStart, tomorrow, "last4Weeks"),
    last12Weeks: range(last12WeeksStart, tomorrow, "last12Weeks"),
    lastMonth: range(lastMonthStart, month, "lastMonth"),
    last6Months: range(last6MonthsStart, tomorrow, "last6Months"),
    all: range(null, null, "all"),
  };
}

export function usageRangePresets(): UsageRangePreset[] {
  return [...PRESETS];
}

export function customUsageRange(from: Date, to: Date): UsageDashboardRange {
  const start = startOfDay(from);
  const end = new Date(to.getFullYear(), to.getMonth(), to.getDate() + 1);
  return range(start, end, "custom");
}

/** 本地日历日键 `YYYY-MM-DD`。 */
export function usageDayKey(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/** 把 `YYYY-MM-DD` 还原成本地 0 点日期。 */
export function usageParseDayKey(key: string): Date {
  const [year, month, day] = key.split("-").map(Number);
  return new Date(year ?? 1970, (month ?? 1) - 1, day ?? 1);
}

/**
 * 日桶归并到所属周期起点：周起点为周一，月起点为自然月 1 号。
 * 底层桶始终是日桶，周／月只在读取时归并（对齐 cc-bar `bucketStart`）。
 */
export function usageBucketStart(day: Date, granularity: UsageGranularity): Date {
  const date = startOfDay(day);
  if (granularity === "week") {
    const offset = (date.getDay() + 6) % 7;
    return new Date(date.getFullYear(), date.getMonth(), date.getDate() - offset);
  }
  if (granularity === "month") {
    return new Date(date.getFullYear(), date.getMonth(), 1);
  }
  return date;
}

/** 该粒度下可选的时间范围：当前／上一个／近 N 个／全部／自定义。日粒度另保留本周／本月／本年。 */
export function usageGranularityPresets(granularity: UsageGranularity): UsageRangePreset[] {
  switch (granularity) {
    case "week":
      return ["thisWeek", "lastWeek", "last4Weeks", "last12Weeks", "all"];
    case "month":
      return ["thisMonth", "lastMonth", "last6Months", "thisYear", "all"];
    default:
      return [
        "today",
        "yesterday",
        "last7Days",
        "last30Days",
        "thisWeek",
        "thisMonth",
        "thisYear",
        "all",
      ];
  }
}

/** 范围末日（半开区间 `to` 往回退一天）。 */
function lastDayOf(value: UsageDashboardRange): Date | null {
  if (!value.to) return null;
  const to = new Date(value.to);
  return new Date(to.getFullYear(), to.getMonth(), to.getDate() - 1);
}

/**
 * 所选范围只落在一个当前粒度周期内时，图表扩展为近 N 个周期的上下文窗口
 * （日 30、周／月 14），范围内的柱子高亮、其余降透明；KPI 与其他面板口径不变。
 * 按桶归属判断而不是按时长，夏令时与月份天数差都不影响结果。
 */
export function usageChartUsesContext(
  value: UsageDashboardRange,
  granularity: UsageGranularity = "day",
): boolean {
  if (!value.from || !value.to) return false;
  const from = new Date(value.from);
  const last = lastDayOf(value);
  if (!last || last < startOfDay(from)) return false;
  return (
    usageBucketStart(from, granularity).getTime() === usageBucketStart(last, granularity).getTime()
  );
}

export function usageChartRange(
  value: UsageDashboardRange,
  granularity: UsageGranularity = "day",
): UsageDashboardRange {
  if (!usageChartUsesContext(value, granularity) || !value.from) return value;

  const periods = CONTEXT_WINDOW_PERIODS[granularity];
  const start = usageBucketStart(new Date(value.from), granularity);
  const contextFrom =
    granularity === "week"
      ? new Date(start.getFullYear(), start.getMonth(), start.getDate() - 7 * (periods - 1))
      : granularity === "month"
        ? new Date(start.getFullYear(), start.getMonth() - (periods - 1), 1)
        : new Date(start.getFullYear(), start.getMonth(), start.getDate() - (periods - 1));
  return { ...value, from: toIso(contextFrom) };
}

/** 将后端半开区间还原成日期选择器使用的两个本地日历日。 */
export function usageDatePickerRange(value: UsageDashboardRange): [Date, Date] | null {
  if (!value.from || !value.to) return null;

  const from = new Date(value.from);
  const exclusiveEnd = new Date(value.to);
  const to = new Date(
    exclusiveEnd.getFullYear(),
    exclusiveEnd.getMonth(),
    exclusiveEnd.getDate() - 1,
  );

  return [startOfDay(from), startOfDay(to)];
}

/**
 * 前一个等长区间（用于 KPI delta 对比）。`all` / `custom` 无法确定有业务意义的
 * 前区间，返回 `null`（对齐 cc-bar `StatsRange.previousBounds`）。例如「本周」取上一自然周。
 */
export function usagePreviousRange(
  value: UsageDashboardRange,
): Pick<UsageDashboardRange, "preset" | "from" | "to"> | null {
  if (value.preset === "all" || value.preset === "custom") return null;
  if (!value.from || !value.to) return null;
  const from = new Date(value.from);
  const to = new Date(value.to);
  const length = to.getTime() - from.getTime();
  if (!Number.isFinite(length) || length <= 0) return null;

  // 进行中的本周／本月／本年：对比上一周期从起点起的同样天数，不越过本周期起点。
  const inProgress: Partial<Record<UsageDashboardRange["preset"], (d: Date) => Date>> = {
    thisWeek: (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate() - 7),
    thisMonth: (d) => new Date(d.getFullYear(), d.getMonth() - 1, d.getDate()),
    thisYear: (d) => new Date(d.getFullYear() - 1, d.getMonth(), d.getDate()),
  };
  const shift = inProgress[value.preset];
  if (shift) {
    const previousStart = shift(from);
    const elapsedDays = Math.round(length / 86_400_000);
    const previousEnd = new Date(
      previousStart.getFullYear(),
      previousStart.getMonth(),
      previousStart.getDate() + elapsedDays,
    );
    return {
      preset: "custom",
      from: previousStart.toISOString(),
      to: new Date(Math.min(previousEnd.getTime(), from.getTime())).toISOString(),
    };
  }

  const previousEnd = from.getTime();
  return {
    preset: "custom",
    from: new Date(previousEnd - length).toISOString(),
    to: new Date(previousEnd).toISOString(),
  };
}

/**
 * `to` 使用本地明日零点而不是当前时刻：扫描期间新写入的今日记录仍落在同一查询范围内。
 * 先用本地日历构造边界，再转 ISO UTC 交给 Rust，DST 切换日也不会硬算 24 小时。
 */
export function usageCostRanges(now: Date = new Date()): UsageCostRanges {
  const ranges = usageDashboardRanges(now);

  return {
    today: { from: ranges.today.from ?? "", to: ranges.today.to ?? "" },
    week: { from: ranges.thisWeek.from ?? "", to: ranges.thisWeek.to ?? "" },
  };
}

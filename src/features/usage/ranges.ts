import type { UsageDashboardRange } from "./contracts";

/** Popover 的两个固定时间范围；与 cc-bar 一样，本周从本地日历周一开始。 */
export interface UsageCostRanges {
  today: { from: string; to: string };
  week: { from: string; to: string };
}

export type UsageRangePreset = Exclude<UsageDashboardRange["preset"], "custom">;

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

function range(from: Date | null, to: Date | null, preset: UsageDashboardRange["preset"]): UsageDashboardRange {
  return { preset, from: from ? toIso(from) : null, to: to ? toIso(to) : null };
}

/** 所有预设都使用本地日历边界，再交给 Rust 做 UTC 查询。 */
export function usageDashboardRanges(now: Date = new Date()): Record<UsageRangePreset, UsageDashboardRange> {
  const today = startOfDay(now);
  const tomorrow = new Date(today.getFullYear(), today.getMonth(), today.getDate() + 1);
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  const daysSinceMonday = (today.getDay() + 6) % 7;
  const monday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - daysSinceMonday);
  const month = new Date(today.getFullYear(), today.getMonth(), 1);
  const year = new Date(today.getFullYear(), 0, 1);
  const last7Days = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 6);
  const last30Days = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 29);

  return {
    today: range(today, tomorrow, "today"),
    yesterday: range(yesterday, today, "yesterday"),
    thisWeek: range(monday, tomorrow, "thisWeek"),
    thisMonth: range(month, tomorrow, "thisMonth"),
    thisYear: range(year, tomorrow, "thisYear"),
    last7Days: range(last7Days, tomorrow, "last7Days"),
    last30Days: range(last30Days, tomorrow, "last30Days"),
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

/** 将后端半开区间还原成日期选择器使用的两个本地日历日。 */
export function usageDatePickerRange(
  value: UsageDashboardRange,
): [Date, Date] | null {
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

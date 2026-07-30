/** Popover 的两个固定时间范围；与 cc-bar 一样，本周从本地日历周一开始。 */
export interface UsageCostRanges {
  today: { from: string; to: string };
  week: { from: string; to: string };
}

/**
 * `to` 使用本地明日零点而不是当前时刻：扫描期间新写入的今日记录仍落在同一查询范围内。
 * 先用本地日历构造边界，再转 ISO UTC 交给 Rust，DST 切换日也不会硬算 24 小时。
 */
export function usageCostRanges(now: Date = new Date()): UsageCostRanges {
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const startOfTomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  const daysSinceMonday = (startOfToday.getDay() + 6) % 7;
  const startOfWeek = new Date(
    startOfToday.getFullYear(),
    startOfToday.getMonth(),
    startOfToday.getDate() - daysSinceMonday,
  );

  return {
    today: {
      from: startOfToday.toISOString(),
      to: startOfTomorrow.toISOString(),
    },
    week: {
      from: startOfWeek.toISOString(),
      to: startOfTomorrow.toISOString(),
    },
  };
}

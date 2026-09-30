import { describe, expect, it } from "vitest";

import {
  customUsageRange,
  usageCostRanges,
  usageBucketStart,
  usageChartRange,
  usageDashboardRanges,
  usageDatePickerRange,
  usageDayKey,
  usageGranularityPresets,
  usagePreviousRange,
  usageRangePresets,
} from "./ranges";

describe("usageCostRanges", () => {
  it("uses local day boundaries and a Monday week start", () => {
    const now = new Date(2026, 6, 29, 15, 42);
    const ranges = usageCostRanges(now);
    const today = new Date(ranges.today.from);
    const tomorrow = new Date(ranges.today.to);
    const week = new Date(ranges.week.from);

    expect([today.getFullYear(), today.getMonth(), today.getDate(), today.getHours()]).toEqual([
      2026, 6, 29, 0,
    ]);
    expect([
      tomorrow.getFullYear(),
      tomorrow.getMonth(),
      tomorrow.getDate(),
      tomorrow.getHours(),
    ]).toEqual([2026, 6, 30, 0]);
    expect(week.getDay()).toBe(1);
    expect([week.getFullYear(), week.getMonth(), week.getDate(), week.getHours()]).toEqual([
      2026, 6, 27, 0,
    ]);
  });

  it("keeps Sunday inside the week that began six days earlier", () => {
    const sunday = new Date(2026, 7, 2, 12);
    const week = new Date(usageCostRanges(sunday).week.from);

    expect(week.getDay()).toBe(1);
    expect(week.getDate()).toBe(27);
  });
});

describe("usageDashboardRanges", () => {
  it("exposes the eight agreed presets and an all-time null boundary", () => {
    expect(usageRangePresets()).toEqual([
      "today",
      "yesterday",
      "thisWeek",
      "thisMonth",
      "thisYear",
      "last7Days",
      "last30Days",
      "all",
    ]);
    expect(usageDashboardRanges(new Date(2026, 6, 29)).all).toEqual({
      preset: "all",
      from: null,
      to: null,
    });
  });

  it("uses an exclusive local midnight for a custom end date", () => {
    const range = customUsageRange(new Date(2026, 6, 1, 16), new Date(2026, 6, 30, 9));
    const from = new Date(range.from ?? "");
    const to = new Date(range.to ?? "");

    expect([from.getDate(), from.getHours()]).toEqual([1, 0]);
    expect([to.getDate(), to.getHours()]).toEqual([31, 0]);
  });

  it("maps a preset back to the same date-only range shown by the picker", () => {
    const range = usageDashboardRanges(new Date(2026, 6, 29)).today;
    const dates = usageDatePickerRange(range);

    expect(dates?.map((date) => [date.getDate(), date.getHours()])).toEqual([
      [29, 0],
      [29, 0],
    ]);
  });
});

describe("usageChartRange", () => {
  it("adds a 30-day local calendar context to a single-day range", () => {
    const today = usageDashboardRanges(new Date(2026, 6, 29, 15)).today;
    const chart = usageChartRange(today);

    expect(new Date(chart.from ?? "").toDateString()).toBe(new Date(2026, 5, 30).toDateString());
    expect(chart.to).toBe(today.to);
  });

  it("adds a 14-period context to a single week or month at that granularity", () => {
    const now = new Date(2026, 6, 29, 15);
    const lastWeek = usageDashboardRanges(now).lastWeek;
    const weekChart = usageChartRange(lastWeek, "week");
    // 上周一 7-20 往前 13 周 = 4-20
    expect(new Date(weekChart.from ?? "").toDateString()).toBe(
      new Date(2026, 3, 20).toDateString(),
    );

    const thisMonth = usageDashboardRanges(now).thisMonth;
    const monthChart = usageChartRange(thisMonth, "month");
    expect(new Date(monthChart.from ?? "").toDateString()).toBe(
      new Date(2025, 5, 1).toDateString(),
    );
  });

  it("does not add context when the range spans several periods", () => {
    const now = new Date(2026, 6, 29);
    const last4Weeks = usageDashboardRanges(now).last4Weeks;
    expect(usageChartRange(last4Weeks, "week")).toEqual(last4Weeks);
    expect(usageChartRange(usageDashboardRanges(now).all, "day")).toEqual(
      usageDashboardRanges(now).all,
    );
  });

  it("keeps multi-day ranges unchanged", () => {
    const month = usageDashboardRanges(new Date(2026, 6, 29)).thisMonth;
    expect(usageChartRange(month)).toEqual(month);
  });
});

describe("granularity helpers", () => {
  it("buckets days into Monday weeks and calendar months", () => {
    const wednesday = new Date(2026, 6, 29);
    expect(usageBucketStart(wednesday, "day").getDate()).toBe(29);
    expect(usageBucketStart(wednesday, "week").getDate()).toBe(27);
    const sunday = new Date(2026, 6, 26);
    expect(usageBucketStart(sunday, "week").getDate()).toBe(20);
    expect(usageDayKey(usageBucketStart(wednesday, "month"))).toBe("2026-07-01");
  });

  it("offers a per-granularity preset list that always includes all", () => {
    expect(usageGranularityPresets("week")).toEqual([
      "thisWeek",
      "lastWeek",
      "last4Weeks",
      "last12Weeks",
      "all",
    ]);
    expect(usageGranularityPresets("month")).toContain("last6Months");
    expect(usageGranularityPresets("day")).toContain("last30Days");
  });

  it("builds the extra week and month presets from local calendar boundaries", () => {
    const ranges = usageDashboardRanges(new Date(2026, 6, 29));
    expect(usageDayKey(new Date(ranges.lastWeek.from ?? ""))).toBe("2026-07-20");
    expect(usageDayKey(new Date(ranges.lastWeek.to ?? ""))).toBe("2026-07-27");
    expect(usageDayKey(new Date(ranges.last12Weeks.from ?? ""))).toBe("2026-05-11");
    expect(usageDayKey(new Date(ranges.lastMonth.from ?? ""))).toBe("2026-06-01");
    expect(usageDayKey(new Date(ranges.lastMonth.to ?? ""))).toBe("2026-07-01");
    expect(usageDayKey(new Date(ranges.last6Months.from ?? ""))).toBe("2026-02-01");
  });
});

describe("usagePreviousRange", () => {
  it("compares an in-progress week, month or year with the same elapsed days of the previous one", () => {
    const now = new Date(2026, 6, 29); // 周三
    const ranges = usageDashboardRanges(now);
    const week = usagePreviousRange(ranges.thisWeek);
    expect([
      usageDayKey(new Date(week?.from ?? "")),
      usageDayKey(new Date(week?.to ?? "")),
    ]).toEqual(["2026-07-20", "2026-07-23"]);
    const month = usagePreviousRange(ranges.thisMonth);
    expect([
      usageDayKey(new Date(month?.from ?? "")),
      usageDayKey(new Date(month?.to ?? "")),
    ]).toEqual(["2026-06-01", "2026-06-30"]);
  });

  it("returns an equal-length range immediately before the current one", () => {
    const now = new Date(2026, 6, 29);
    const week = usageDashboardRanges(now).last7Days;
    const previous = usagePreviousRange(week);

    expect(previous).not.toBeNull();
    const from = new Date(previous?.from ?? "");
    const to = new Date(previous?.to ?? "");
    const currentFrom = new Date(week.from ?? "");
    expect(new Date(previous?.to ?? "").getTime()).toBe(currentFrom.getTime());
    expect(to.getTime() - from.getTime()).toBe(
      new Date(week.to ?? "").getTime() - currentFrom.getTime(),
    );
  });

  it("returns null for all-time and custom ranges", () => {
    expect(usagePreviousRange(usageDashboardRanges(new Date(2026, 6, 29)).all)).toBeNull();
    expect(
      usagePreviousRange(customUsageRange(new Date(2026, 6, 1), new Date(2026, 6, 30))),
    ).toBeNull();
  });
});

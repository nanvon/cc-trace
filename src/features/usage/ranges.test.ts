import { describe, expect, it } from "vitest";

import {
  customUsageRange,
  usageCostRanges,
  usageDashboardRanges,
  usageDatePickerRange,
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

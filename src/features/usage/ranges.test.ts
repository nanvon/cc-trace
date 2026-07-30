import { describe, expect, it } from "vitest";

import { usageCostRanges } from "./ranges";

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

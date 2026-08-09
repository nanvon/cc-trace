import { describe, expect, it } from "vitest";

import {
  compactAge,
  compactDuration,
  compactReset,
  formatCompactTokens,
  formatCompactUsdNanos,
  formatCountdown,
  formatPast,
  formatPercent,
  formatUsdNanos,
  splitPercent,
} from "./format";

const NOW = new Date("2026-07-25T12:00:00Z");
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

describe("formatPercent", () => {
  it("rounds to whole numbers", () => {
    expect(formatPercent("en", 73.4)).toBe("73%");
    expect(formatPercent("zh-CN", 73.6)).toBe("74%");
  });

  it("never rounds a small remainder down to zero", () => {
    expect(formatPercent("en", 0.4)).toBe("<1%");
  });

  it("shows a real zero as zero", () => {
    expect(formatPercent("en", 0)).toBe("0%");
  });
});

describe("splitPercent", () => {
  it("separates the number from the percent sign", () => {
    expect(splitPercent("en", 84)).toEqual({ value: "84", unit: "%" });
    expect(splitPercent("zh-CN", 84)).toEqual({ value: "84", unit: "%" });
  });

  it("keeps the small-remainder marker on the number side", () => {
    expect(splitPercent("en", 0.4)).toEqual({ value: "<1", unit: "%" });
  });

  it("stays consistent with the joined form", () => {
    const parts = splitPercent("en", 26);
    expect(`${parts.value}${parts.unit}`).toBe(formatPercent("en", 26));
  });
});

describe("USD nanos", () => {
  it("keeps the compact popover reading stable", () => {
    expect(formatCompactUsdNanos("en", 0)).toBe("$0");
    expect(formatCompactUsdNanos("en", 420_000_000)).toBe("<$1");
    expect(formatCompactUsdNanos("en", 1_600_000_000)).toBe("$2");
    expect(formatCompactUsdNanos("en", 12_345_000_000_000)).toBe("$12,345");
  });

  it("keeps cents in the accessible full amount", () => {
    expect(formatUsdNanos("en", 1_234_000_000)).toBe("$1.23");
  });
});

describe("formatCompactTokens", () => {
  it("shows a real zero and small counts as-is", () => {
    expect(formatCompactTokens("en", 0)).toBe("0");
    expect(formatCompactTokens("en", 999)).toBe("999");
  });

  it("uses the locale's compact units", () => {
    expect(formatCompactTokens("en", 1_234)).toBe("1.2K");
    expect(formatCompactTokens("en", 67_700)).toBe("67.7K");
    expect(formatCompactTokens("en", 1_234_567)).toBe("1.2M");
    expect(formatCompactTokens("en", 12_345_678_901)).toBe("12.3B");
    expect(formatCompactTokens("zh-CN", 67_700)).toBe("6.8万");
    expect(formatCompactTokens("zh-CN", 123_456_789)).toBe("1.2亿");
  });
});

describe("compactDuration", () => {
  it("never shows seconds", () => {
    expect(compactDuration(30_000)).toBe("<1m");
    expect(compactDuration(0)).toBe("<1m");
  });

  it("uses minutes inside the first hour", () => {
    expect(compactDuration(43 * MINUTE)).toBe("43m");
  });

  it("keeps at most the two highest units", () => {
    expect(compactDuration(HOUR + 42 * MINUTE)).toBe("1h42m");
    expect(compactDuration(6 * DAY + 2 * HOUR + 59 * MINUTE)).toBe("6d2h");
  });

  it("drops an empty lower unit instead of writing a zero", () => {
    expect(compactDuration(4 * HOUR)).toBe("4h");
    expect(compactDuration(3 * DAY)).toBe("3d");
  });

  // 定宽是这个格式存在的理由，上界必须被守住：`23h59m` 是最长的分支
  it("stays within six characters across every branch", () => {
    const samples = [
      30_000,
      MINUTE,
      59 * MINUTE,
      HOUR + 42 * MINUTE,
      23 * HOUR + 59 * MINUTE,
      3 * DAY + 22 * HOUR,
      400 * DAY,
    ];
    for (const ms of samples) {
      expect(compactDuration(ms).length).toBeLessThanOrEqual(6);
    }
  });

  it("treats an elapsed deadline as under a minute, not a negative", () => {
    expect(compactDuration(-5 * HOUR)).toBe("<1m");
  });
});

describe("compactReset", () => {
  it("counts down to the reset moment", () => {
    const iso = new Date(NOW.getTime() + 3 * DAY + 22 * HOUR).toISOString();
    expect(compactReset(iso, NOW)).toBe("3d22h");
  });
});

describe("compactAge", () => {
  it("keeps a sub-second age as justNow", () => {
    const iso = new Date(NOW.getTime() - 500).toISOString();
    expect(compactAge(iso, NOW).kind).toBe("justNow");
  });

  it("counts seconds inside the first minute", () => {
    const iso = new Date(NOW.getTime() - 30_000).toISOString();
    expect(compactAge(iso, NOW)).toEqual({ kind: "compact", text: "30s" });
  });

  it("reports elapsed time in the compact form past one minute", () => {
    const iso = new Date(NOW.getTime() - MINUTE).toISOString();
    expect(compactAge(iso, NOW)).toEqual({ kind: "compact", text: "1m" });
  });
});

describe("formatPast", () => {
  it("collapses the last minute into a single phrase", () => {
    const result = formatPast("en", new Date(NOW.getTime() - 30_000).toISOString(), NOW);
    expect(result.kind).toBe("justNow");
  });

  it("uses relative wording inside a day", () => {
    const result = formatPast("en", new Date(NOW.getTime() - 5 * 60_000).toISOString(), NOW);
    expect(result).toEqual({ kind: "relative", text: "5 minutes ago" });
  });

  it("switches to an absolute date past 24 hours", () => {
    const result = formatPast("en", new Date(NOW.getTime() - 30 * 3_600_000).toISOString(), NOW);
    expect(result.kind).toBe("absolute");
  });
});

describe("formatCountdown", () => {
  it("reports sub-minute waits without a seconds counter", () => {
    const result = formatCountdown("en", new Date(NOW.getTime() + 20_000).toISOString(), NOW);
    expect(result.kind).toBe("underOneMinute");
  });

  it("counts down in minutes", () => {
    const result = formatCountdown("en", new Date(NOW.getTime() + 4 * 60_000).toISOString(), NOW);
    expect(result).toEqual({ kind: "relative", text: "in 4 minutes" });
  });

  it("treats an elapsed retry window as available now", () => {
    const result = formatCountdown("en", new Date(NOW.getTime() - 1000).toISOString(), NOW);
    expect(result.kind).toBe("justNow");
  });
});

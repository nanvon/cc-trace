import { describe, expect, it } from "vitest";

import { formatCountdown, formatPast, formatPercent } from "./format";

const NOW = new Date("2026-07-25T12:00:00Z");

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

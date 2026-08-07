import { describe, expect, it } from "vitest";

import type { QuotaHistoryEvent } from "../usage/contracts";
import { activeSeriesByProvider, groupSeries, latestEvent, seriesKey, todayDelta } from "./history";

function event(
  overrides: Partial<QuotaHistoryEvent> &
    Pick<QuotaHistoryEvent, "observedAt" | "remainingPercent">,
): QuotaHistoryEvent {
  return {
    provider: "codex",
    identityKey: "identity",
    windowKind: "fiveHour",
    windowId: "window",
    ...overrides,
  };
}

describe("quota history series", () => {
  it("keeps one series per provider, identity, window kind and window id", () => {
    const events = [
      event({ observedAt: "2026-07-30T00:00:00Z", remainingPercent: 80 }),
      event({
        provider: "claude",
        identityKey: "claude-identity",
        windowKind: "weekly",
        windowId: null,
        observedAt: "2026-07-30T01:00:00Z",
        remainingPercent: 90,
      }),
      event({ observedAt: "2026-07-31T00:00:00Z", remainingPercent: 70 }),
    ];

    const series = groupSeries(events);
    expect(series).toHaveLength(2);
    const codex = series.find((group) => group.provider === "codex");
    expect(codex?.points).toHaveLength(2);
    expect(codex?.points[0].remainingPercent).toBe(80);
    expect(codex?.points[1].remainingPercent).toBe(70);
  });

  it("sorts points inside a series chronologically", () => {
    const series = groupSeries([
      event({ observedAt: "2026-07-30T02:00:00Z", remainingPercent: 70 }),
      event({ observedAt: "2026-07-30T00:00:00Z", remainingPercent: 80 }),
      event({ observedAt: "2026-07-30T01:00:00Z", remainingPercent: 75 }),
    ]);

    expect(series[0].points.map((point) => point.remainingPercent)).toEqual([80, 75, 70]);
  });

  it("uses the full series identity for the dedup key", () => {
    const a = event({ observedAt: "2026-07-30T00:00:00Z", remainingPercent: 80 });
    const b = event({
      identityKey: "another-identity",
      observedAt: "2026-07-30T00:00:00Z",
      remainingPercent: 80,
    });
    expect(seriesKey(a)).not.toBe(seriesKey(b));
  });

  it("picks the active series as the one with the provider's latest event", () => {
    const events = [
      event({ observedAt: "2026-07-29T00:00:00Z", remainingPercent: 90 }),
      event({
        identityKey: "older-identity",
        observedAt: "2026-07-28T00:00:00Z",
        remainingPercent: 60,
      }),
      event({ observedAt: "2026-07-30T00:00:00Z", remainingPercent: 80 }),
    ];

    const active = activeSeriesByProvider(events);
    const series = active.get("codex");
    expect(series?.identityKey).toBe("identity");
    expect(latestEvent(series!).remainingPercent).toBe(80);
    expect(active.get("claude")).toBeUndefined();
  });

  it("exposes latest and today delta per active series", () => {
    const events = [
      event({ observedAt: "2026-07-30T00:00:00Z", remainingPercent: 80 }),
      event({ observedAt: "2026-07-30T06:00:00Z", remainingPercent: 72 }),
      event({ observedAt: "2026-07-31T12:00:00Z", remainingPercent: 60 }),
    ];
    const active = activeSeriesByProvider(events);
    const series = active.get("codex")!;

    expect(latestEvent(series).remainingPercent).toBe(60);
    const now = new Date("2026-07-31T20:00:00");
    expect(todayDelta(series, now)).toBe(0);
  });

  it("returns a null today delta when there is no event today", () => {
    const series = groupSeries([
      event({ observedAt: "2026-07-30T00:00:00Z", remainingPercent: 80 }),
      event({ observedAt: "2026-07-30T06:00:00Z", remainingPercent: 72 }),
    ])[0];
    const now = new Date("2026-08-01T10:00:00");
    expect(todayDelta(series, now)).toBeNull();
  });
});

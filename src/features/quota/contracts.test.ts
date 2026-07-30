import { describe, expect, it } from "vitest";

import { primaryWindow, secondaryWindows, type QuotaSnapshot, type QuotaWindow } from "./contracts";

function window(id: string, isPrimary: boolean): QuotaWindow {
  return {
    id,
    kind: "weekly",
    displayName: null,
    usedPercent: 25,
    remainingPercent: 75,
    resetsAt: null,
    windowSeconds: null,
    isActive: true,
    isPrimary,
  };
}

describe("quota window display order", () => {
  it("always uses the first returned window as primary", () => {
    const first = window("first", false);
    const legacyPrimary = window("legacy-primary", true);
    const snapshot: QuotaSnapshot = {
      windows: [first, legacyPrimary],
      capturedAt: "2026-07-28T00:00:00Z",
    };

    expect(primaryWindow(snapshot)).toBe(first);
    expect(secondaryWindows(snapshot)).toEqual([legacyPrimary]);
  });

  it("returns no primary or secondary windows for an empty snapshot", () => {
    const snapshot: QuotaSnapshot = {
      windows: [],
      capturedAt: "2026-07-28T00:00:00Z",
    };

    expect(primaryWindow(snapshot)).toBeNull();
    expect(secondaryWindows(snapshot)).toEqual([]);
  });
});

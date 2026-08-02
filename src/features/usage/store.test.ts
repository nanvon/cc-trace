import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getUsageScanStatus, getUsageSummary } from "./api";
import type { UsageScanStatus, UsageSummary } from "./contracts";
import { usageChartRange, usageDashboardRanges } from "./ranges";
import { useUsageStore } from "./store";

vi.mock("./api", () => ({
  getUsageScanStatus: vi.fn(),
  getUsageSummary: vi.fn(),
}));

const idleStatus = (finishedAt: string | null): UsageScanStatus => ({
  state: "idle",
  currentSource: null,
  discoveredFiles: 0,
  completedFiles: 0,
  bytesRead: 0,
  insertedEntries: 0,
  duplicateEntries: 0,
  invalidLines: 0,
  failedFiles: 0,
  partialFailure: false,
  cancelled: false,
  startedAt: null,
  finishedAt,
});

const emptySummary: UsageSummary = {
  rows: [],
  entryCount: 0,
  tokens: {
    uncachedInputTokens: 0,
    outputTokens: 0,
    reasoningOutputTokens: 0,
    cacheReadInputTokens: 0,
    cacheWrite5mInputTokens: 0,
    cacheWrite1hInputTokens: 0,
    inputTokens: 0,
    totalTokens: 0,
  },
  fast: {
    rawTokens: 0,
    billingEquivalentTokens: "0",
    minimumMultiplier: null,
    maximumMultiplier: null,
    hasUnpricedEquivalent: false,
  },
  cost: {
    apiEquivalentCostNanos: 0,
    pricedEntries: 0,
    unpricedEntries: 0,
    assumedGeoEntries: 0,
    pricingFingerprint: null,
  },
};

describe("usage store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.mocked(getUsageScanStatus).mockReset();
    vi.mocked(getUsageSummary).mockReset();
    vi.mocked(getUsageSummary).mockResolvedValue(emptySummary);
  });

  it("shows loading before the first status and summary read finishes", async () => {
    vi.mocked(getUsageScanStatus).mockResolvedValue(idleStatus(null));
    const store = useUsageStore();

    expect(store.loading).toBe(true);
    await store.load();
    expect(store.loading).toBe(false);
  });

  it("reloads summaries when a scan starts and finishes between polls", async () => {
    vi.mocked(getUsageScanStatus)
      .mockResolvedValueOnce(idleStatus(null))
      .mockResolvedValueOnce(idleStatus("2026-07-30T10:00:00Z"));
    const store = useUsageStore();

    await store.load();
    expect(getUsageSummary).toHaveBeenCalledTimes(2);

    await store.poll();
    expect(getUsageSummary).toHaveBeenCalledTimes(4);
  });

  it("loads source, daily, and model summaries for the main usage page", async () => {
    vi.mocked(getUsageScanStatus).mockResolvedValue(idleStatus("2026-07-30T10:00:00Z"));
    const store = useUsageStore();

    await store.loadDashboard(usageDashboardRanges(new Date(2026, 6, 30)).thisMonth);

    expect(vi.mocked(getUsageSummary).mock.calls.map(([query]) => query.groupBy)).toEqual([
      "source",
      "day",
      "day",
      "model",
      "model",
    ]);
    expect(store.dashboardLoaded).toBe(true);
    expect(store.dashboardLoading).toBe(false);
    expect(store.dashboardUnavailable).toBe(false);
  });

  it("uses the wider context only for daily summaries in a single-day range", async () => {
    vi.mocked(getUsageScanStatus).mockResolvedValue(idleStatus("2026-07-30T10:00:00Z"));
    const store = useUsageStore();
    const today = usageDashboardRanges(new Date(2026, 6, 30)).today;

    await store.loadDashboard(today);

    const queries = vi.mocked(getUsageSummary).mock.calls.map(([query]) => query);
    expect(queries[0]?.filter.from).toBe(today.from);
    expect(queries[1]?.filter.from).toBe(usageChartRange(today).from);
    expect(queries[2]?.filter.from).toBe(usageChartRange(today).from);
    expect(queries[3]?.filter.from).toBe(today.from);
    expect(queries[4]?.filter.from).toBe(today.from);
  });
});

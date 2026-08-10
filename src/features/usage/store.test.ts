import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getUsageScanStatus, getUsageSummary } from "./api";
import type { UsageScanStatus, UsageSummary } from "./contracts";
import { useSettingsStore } from "../settings/store";
import { usageChartRange, usageDashboardRanges, usagePreviousRange } from "./ranges";
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

  it("loads source, daily, and model summaries for the visible services", async () => {
    vi.mocked(getUsageScanStatus).mockResolvedValue(idleStatus("2026-07-30T10:00:00Z"));
    const store = useUsageStore();

    await store.loadDashboard(usageDashboardRanges(new Date(2026, 6, 30)).thisMonth);

    expect(vi.mocked(getUsageSummary).mock.calls.map(([query]) => query.groupBy)).toEqual([
      "source",
      "source",
      "day",
      "day",
      "day",
      "day",
      "model",
      "model",
      "model",
      "model",
    ]);
    expect(store.dashboardLoaded).toBe(true);
    expect(store.dashboardLoading).toBe(false);
    expect(store.dashboardUnavailable).toBe(false);
  });

  it("filters visible services from the dashboard queries", async () => {
    vi.mocked(getUsageScanStatus).mockResolvedValue(idleStatus("2026-07-30T10:00:00Z"));
    const settings = useSettingsStore();
    settings.adopt({
      schemaVersion: 1,
      language: "zh-CN",
      appearance: "system",
      refreshInterval: "2m",
      launchAtLogin: false,
      privacyMode: false,
      showServiceStatus: true,
      onboarding: { completed: true, completedAt: null },
      usageServiceVisibility: { codex: true, claude: true, pi: false, opencode: false },
    });
    const store = useUsageStore();

    await store.loadDashboard(usageDashboardRanges(new Date(2026, 6, 30)).thisMonth);

    expect(store.visibleSources).toEqual(["codex", "claude"]);
    const sources = vi
      .mocked(getUsageSummary)
      .mock.calls.slice(2)
      .map(([query]) => query.filter.source);
    expect(sources).toEqual(["codex", "claude", "codex", "claude"]);
  });

  it("narrows dashboard queries to the selected source (global filter, not persisted)", async () => {
    vi.mocked(getUsageScanStatus).mockResolvedValue(idleStatus("2026-07-30T10:00:00Z"));
    const store = useUsageStore();

    expect(store.sourceFilter).toBe("all");
    store.selectSource("claude");
    expect(store.sourceFilter).toBe("claude");
    expect(store.dashboardSources).toEqual(["claude"]);

    await store.loadDashboard(usageDashboardRanges(new Date(2026, 6, 30)).thisMonth);

    const sources = vi
      .mocked(getUsageSummary)
      .mock.calls.slice(2)
      .map(([query]) => query.filter.source);
    expect(sources).toEqual(["claude", "claude"]);
    expect(store.visibleSourceSummary).toBeNull();
  });

  it("falls back to all when the selected source is turned off in settings", async () => {
    vi.mocked(getUsageScanStatus).mockResolvedValue(idleStatus("2026-07-30T10:00:00Z"));
    const settings = useSettingsStore();
    settings.adopt({
      schemaVersion: 1,
      language: "zh-CN",
      appearance: "system",
      refreshInterval: "2m",
      launchAtLogin: false,
      privacyMode: false,
      showServiceStatus: true,
      onboarding: { completed: true, completedAt: null },
      usageServiceVisibility: { codex: true, claude: false, pi: false, opencode: false },
    });
    const store = useUsageStore();

    store.selectSource("claude");
    expect(store.sourceFilter).toBe("all");
    expect(store.sourceFilterOptions).toEqual(["all", "codex"]);
  });

  it("maps day and model summaries correctly when previous range is absent (all)", async () => {
    vi.mocked(getUsageScanStatus).mockResolvedValue(idleStatus("2026-07-30T10:00:00Z"));
    const store = useUsageStore();
    const labeled = (key: string): UsageSummary => ({
      ...emptySummary,
      rows: [
        { key, entryCount: 1, tokens: emptySummary.tokens, fast: emptySummary.fast, cost: emptySummary.cost },
      ],
    });

    vi.mocked(getUsageSummary)
      .mockResolvedValueOnce(labeled("source-all"))
      .mockResolvedValueOnce(labeled("day-codex"))
      .mockResolvedValueOnce(labeled("day-claude"))
      .mockResolvedValueOnce(labeled("day-pi"))
      .mockResolvedValueOnce(labeled("day-opencode"))
      .mockResolvedValueOnce(labeled("model-codex"))
      .mockResolvedValueOnce(labeled("model-claude"))
      .mockResolvedValueOnce(labeled("model-pi"))
      .mockResolvedValueOnce(labeled("model-opencode"));

    await store.loadDashboard(usageDashboardRanges(new Date(2026, 6, 30)).all);

    expect(vi.mocked(getUsageSummary).mock.calls.map(([query]) => query.groupBy)).toEqual([
      "source",
      "day",
      "day",
      "day",
      "day",
      "model",
      "model",
      "model",
      "model",
    ]);
    expect(store.dashboardPrevious).toBeNull();
    expect(store.dashboard.day.codex?.rows[0]?.key).toBe("day-codex");
    expect(store.dashboard.day.claude?.rows[0]?.key).toBe("day-claude");
    expect(store.dashboard.day.pi?.rows[0]?.key).toBe("day-pi");
    expect(store.dashboard.day.opencode?.rows[0]?.key).toBe("day-opencode");
    expect(store.dashboard.model.codex?.rows[0]?.key).toBe("model-codex");
    expect(store.dashboard.model.claude?.rows[0]?.key).toBe("model-claude");
    expect(store.dashboard.model.pi?.rows[0]?.key).toBe("model-pi");
    expect(store.dashboard.model.opencode?.rows[0]?.key).toBe("model-opencode");
  });

  it("uses the wider context only for daily summaries in a single-day range", async () => {
    vi.mocked(getUsageScanStatus).mockResolvedValue(idleStatus("2026-07-30T10:00:00Z"));
    const store = useUsageStore();
    const today = usageDashboardRanges(new Date(2026, 6, 30)).today;

    await store.loadDashboard(today);

    const queries = vi.mocked(getUsageSummary).mock.calls.map(([query]) => query);
    const previous = usagePreviousRange(today);
    expect(queries[0]?.filter.from).toBe(today.from);
    expect(queries[1]?.filter.from).toBe(previous?.from);
    expect(queries[2]?.filter.from).toBe(usageChartRange(today).from);
    expect(queries[3]?.filter.from).toBe(usageChartRange(today).from);
    expect(queries[4]?.filter.from).toBe(usageChartRange(today).from);
    expect(queries[5]?.filter.from).toBe(usageChartRange(today).from);
    expect(queries[6]?.filter.from).toBe(today.from);
    expect(queries[7]?.filter.from).toBe(today.from);
    expect(queries[8]?.filter.from).toBe(today.from);
    expect(queries[9]?.filter.from).toBe(today.from);
  });
});
